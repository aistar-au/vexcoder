use super::super::*;

impl TuiMode {
    pub(crate) fn handle_commands_command(&mut self) {
        self.push_history_line("[commands]".to_string());
        self.push_history_line(
            "[commands] retrieve with /context, /tools, and /generate-tests before opening or mutating files"
                .to_string(),
        );
        let groups = [
            "retrieve + context",
            "edit + inspect",
            "validate + execute",
            "session + control",
        ];
        let mut seen = std::collections::HashSet::new();
        for group in groups {
            let mut group_lines = Vec::new();
            for spec in SLASH_COMMANDS {
                if slash_command_menu_group(spec.id) != group || !seen.insert(spec.display) {
                    continue;
                }
                group_lines.push(format!(
                    "  {:32} — {} [{}]",
                    spec.display,
                    spec.description,
                    slash_command_mode_summary(spec.id)
                ));
            }
            if !group_lines.is_empty() {
                self.push_history_line(format!("[{group}]"));
                for line in group_lines {
                    self.push_history_line(line);
                }
            }
        }
        if !self.custom_commands.is_empty() {
            self.push_history_line("[custom commands]".to_string());
            let custom_command_lines = self
                .custom_commands
                .iter()
                .map(|command| format!("  {:32} — {}", command.display(), command.description))
                .collect::<Vec<_>>();
            for line in custom_command_lines {
                self.push_history_line(line);
            }
        }
    }

    pub(crate) fn handle_mcp_command(&mut self, args: &str) {
        let trimmed = args.trim();
        let Some(snapshot) = self
            .mcp_rollup
            .clone()
            .filter(|snapshot| !snapshot.is_empty())
        else {
            self.push_history_line("[mcp] no MCP servers loaded".to_string());
            return;
        };

        if trimmed.is_empty() || trimmed == "list" {
            self.push_history_line("[mcp]".to_string());
            self.push_history_line(format!(
                "[mcp] {} server(s), {} tool(s) loaded",
                snapshot.servers.len(),
                snapshot.all_tools().len()
            ));
            for server in &snapshot.servers {
                self.push_history_line(format!(
                    "  {} transport={} tools={}",
                    server.name,
                    server.transport,
                    server.tools.len()
                ));
            }
            return;
        }

        let mut parts = trimmed.split_whitespace();
        match (parts.next(), parts.next(), parts.next()) {
            (Some("show"), Some(server_name), None) => {
                let Some(server) = snapshot
                    .servers
                    .iter()
                    .find(|server| server.name == server_name)
                else {
                    self.push_history_line(format!("[mcp] unknown server '{server_name}'"));
                    return;
                };
                self.push_history_line(format!("[mcp:{}]", server.name));
                self.push_history_line(format!(
                    "[mcp:{}] transport={} tools={}",
                    server.name,
                    server.transport,
                    server.tools.len()
                ));
                for tool in &server.tools {
                    self.push_history_line(format!("  {} — {}", tool.full_name, tool.description));
                }
            }
            _ => self.push_history_line("[mcp] usage: /mcp [list|show <server>]".to_string()),
        }
    }

    pub(crate) fn handle_tools_command(&mut self, args: &str) {
        let include_descriptions = match args.trim() {
            "" => false,
            "desc" => true,
            _ => {
                self.push_history_line("[tools] usage: /tools [desc]".to_string());
                return;
            }
        };

        self.push_history_line("[tools]".to_string());
        if let Some(snapshot) = self
            .mcp_rollup
            .clone()
            .filter(|snapshot| !snapshot.is_empty())
        {
            self.push_history_line(format!(
                "[tools] active registry: {} built-in tool(s), {} MCP server(s), {} MCP tool(s)",
                builtin_tool_summaries().len(),
                snapshot.servers.len(),
                snapshot.all_tools().len()
            ));
        } else {
            self.push_history_line("[tools] active registry: built-in tools only".to_string());
        }
        self.push_history_line(
            "[tools] discovery flow: list_files/find_files -> search_content/codebase_search -> read_file"
                .to_string(),
        );
        self.push_history_line(
            "[tools] mutation flow: edit_file/apply_patch/write_file -> git_diff/git_status -> git_add/git_commit"
                .to_string(),
        );

        let groups = ["retrieve", "mutate", "git", "other"];
        let tools = builtin_tool_summaries();
        if let Some(snapshot) = self
            .mcp_rollup
            .clone()
            .filter(|snapshot| !snapshot.is_empty())
        {
            self.push_history_line("[tools:mcp]".to_string());
            for tool in snapshot.all_tools() {
                if include_descriptions {
                    self.push_history_line(format!(
                        "  {:24} — {} [server tool]",
                        tool.full_name, tool.description
                    ));
                } else {
                    self.push_history_line(format!("  {:24} [server tool]", tool.full_name));
                }
            }
        }
        for group in groups {
            let grouped = tools
                .iter()
                .filter(|tool| builtin_tool_menu_group(&tool.name) == group)
                .collect::<Vec<_>>();
            if grouped.is_empty() {
                continue;
            }

            self.push_history_line(format!("[tools:{group}]"));
            for tool in grouped {
                if include_descriptions {
                    self.push_history_line(format!(
                        "  {:24} — {} [{}]",
                        tool.name,
                        tool.description,
                        builtin_tool_usage_hint(&tool.name)
                    ));
                } else {
                    self.push_history_line(format!(
                        "  {:24} {}",
                        tool.name,
                        builtin_tool_usage_hint(&tool.name)
                    ));
                }
            }
        }
    }
    pub(crate) fn handle_usage_command(&mut self, ctx: &RuntimeContext) {
        let usage = ctx.session_tokens_rollup();
        if !usage.has_completed_turns() {
            self.push_history_line("[usage] no turns completed this session".to_string());
            return;
        }

        let turn_estimated = Self::summarize_usage_line_suffix(usage.last_estimated);
        let session_estimated = Self::summarize_usage_line_suffix(usage.estimated);
        self.push_history_line("[usage]".to_string());
        self.push_history_line(format!(
            "  this turn   : {} in / {} out{}",
            usage.last_input, usage.last_output, turn_estimated
        ));
        self.push_history_line(format!(
            "  session     : {} in / {} out{}",
            usage.input, usage.output, session_estimated
        ));
    }
    pub(crate) fn handle_agents_command(&mut self) {
        match crate::app::facade_list_agents(&self.working_dir) {
            Err(_) => {
                self.push_history_line("[agents] failed to read agents config".to_string());
            }
            Ok(listing) if !listing.available => {
                self.push_history_line("[agents] no .vex/agents.toml found".to_string());
            }
            Ok(listing) => {
                self.push_history_line("[agents]".to_string());
                for agent in &listing.agents {
                    self.push_history_line(format!(
                        "  {} profile={} isolation={} max_parallel={} active={}",
                        agent.name,
                        agent.profile,
                        agent.isolation,
                        agent.max_parallel_tasks,
                        agent.live_session_tasks,
                    ));
                }
                if !listing.teams.is_empty() {
                    self.push_history_line("[teams]".to_string());
                    for team in &listing.teams {
                        self.push_history_line(format!(
                            "  {} members={} scheduler={}",
                            team.name,
                            team.members.join(", "),
                            team.scheduler,
                        ));
                    }
                }
            }
        }
    }
    pub(crate) fn handle_delegate_command(&mut self, args: &str) {
        let mut parts = args.trim().splitn(2, char::is_whitespace);
        let Some(first) = parts.next().filter(|s| !s.is_empty()) else {
            self.push_history_line(
                "[delegate] usage: /delegate <agent> <prompt>  or  /delegate team <name> <prompt>"
                    .to_string(),
            );
            return;
        };
        let Some(rest) = parts.next().map(str::trim).filter(|s| !s.is_empty()) else {
            self.push_history_line(
                "[delegate] usage: /delegate <agent> <prompt>  or  /delegate team <name> <prompt>"
                    .to_string(),
            );
            return;
        };

        if first == "team" {
            let mut team_parts = rest.splitn(2, char::is_whitespace);
            let Some(team_name) = team_parts.next().filter(|s| !s.is_empty()) else {
                self.push_history_line(
                    "[delegate] usage: /delegate team <name> <prompt>".to_string(),
                );
                return;
            };
            let Some(prompt) = team_parts.next().map(str::trim).filter(|s| !s.is_empty()) else {
                self.push_history_line(
                    "[delegate] usage: /delegate team <name> <prompt>".to_string(),
                );
                return;
            };
            match crate::app::facade_schedule_team(
                &self.working_dir,
                &self.task_doc.info.id,
                team_name,
                prompt,
            ) {
                Ok(result) => {
                    self.push_history_line(format!(
                        "[delegate] team {} scheduled {} session tasks (scheduler={})",
                        team_name,
                        result.session_task_ids.len(),
                        result.scheduler,
                    ));
                    self.sync_session_tasks_from_disk();
                }
                Err(crate::app::ScheduleTeamError::TeamNotFound) => {
                    self.push_history_line(format!("[delegate] team '{team_name}' not found"));
                }
                Err(crate::app::ScheduleTeamError::AgentsConfigMissing) => {
                    self.push_history_line("[delegate] no .vex/agents.toml found".to_string());
                }
                Err(crate::app::ScheduleTeamError::ParentTaskIdRequired) => {
                    self.push_history_line(
                        "[delegate] no active task — start a task first".to_string(),
                    );
                }
                Err(crate::app::ScheduleTeamError::PromptRequired) => {
                    self.push_history_line("[delegate] prompt must not be empty".to_string());
                }
                Err(err) => {
                    self.push_history_line(format!("[delegate] schedule failed: {err}"));
                }
            }
            return;
        }

        let agent_id = first;
        let prompt = rest;
        match crate::app::facade_delegate_session_task(
            &self.working_dir,
            Some(self.task_doc.info.id.clone()),
            agent_id,
            prompt,
        ) {
            Ok(result) => {
                self.push_history_line(format!(
                    "[delegate] session task {} assigned to {}",
                    result.session_task_id, agent_id,
                ));
                self.sync_session_tasks_from_disk();
            }
            Err(crate::app::DelegateError::AgentNotFound) => {
                self.push_history_line(format!("[delegate] unknown agent '{agent_id}'"));
            }
            Err(crate::app::DelegateError::AgentsConfigMissing) => {
                self.push_history_line("[delegate] no .vex/agents.toml found".to_string());
            }
            Err(crate::app::DelegateError::ParentTaskIdRequired) => {
                self.push_history_line(
                    "[delegate] no active task — start a task first".to_string(),
                );
            }
            Err(crate::app::DelegateError::ConcurrencyLimitReached) => {
                self.push_history_line(format!(
                    "[delegate] agent '{agent_id}' is at its concurrency limit"
                ));
            }
            Err(crate::app::DelegateError::PromptTooLong) => {
                self.push_history_line("[delegate] prompt exceeds maximum length".to_string());
            }
            Err(err) => {
                self.push_history_line(format!("[delegate] failed: {err}"));
            }
        }
    }
    pub(crate) fn handle_watch_command(&mut self, args: &str) {
        let selector = args.trim();
        if selector.is_empty() {
            if self.task_doc.session_tasks.is_empty() {
                self.push_history_line("[watch] no session tasks recorded".to_string());
                return;
            }
            self.push_history_line("[watch] current task session tasks:".to_string());
            let lines: Vec<String> = self
                .task_doc
                .session_tasks
                .iter()
                .map(|task| {
                    format!(
                        "  {} agent={} status={}",
                        task.id, task.agent_id, task.lifecycle_state
                    )
                })
                .collect();
            for line in lines {
                self.push_history_line(line);
            }
            if let Ok(Some(outcome)) =
                crate::app::facade_poll_join(&self.working_dir, &self.task_doc.info.id)
            {
                self.push_history_line(format!(
                    "[watch] join: done={} completed={} failed={} cancelled={}",
                    outcome.all_done, outcome.completed, outcome.failed, outcome.cancelled,
                ));
            }
            return;
        }

        match crate::app::facade_watch_rollup(&self.working_dir, selector) {
            Ok(Some(snap)) => {
                let worktree = snap.worktree_path.unwrap_or_else(|| "shared".to_string());
                match (snap.parent_task_id, snap.agent_id) {
                    (Some(parent), Some(agent)) => {
                        self.push_history_line(format!(
                            "[watch] {} parent={} agent={} status={} worktree={}",
                            snap.id, parent, agent, snap.status, worktree,
                        ));
                    }
                    _ => {
                        self.push_history_line(format!(
                            "[watch] {} status={} worktree={}",
                            snap.id, snap.status, worktree,
                        ));
                        if let Ok(Some(outcome)) =
                            crate::app::facade_poll_join(&self.working_dir, selector)
                        {
                            self.push_history_line(format!(
                                "[watch] join: done={} completed={} failed={} cancelled={}",
                                outcome.all_done,
                                outcome.completed,
                                outcome.failed,
                                outcome.cancelled,
                            ));
                        }
                    }
                }
            }
            Ok(None) => {
                // facade_watch_rollup matches by task-id and session-task UUID.
                // Fall back to searching by agent_id for human-readable selectors.
                let by_agent = TaskState::state_files_from(&self.working_dir)
                    .into_iter()
                    .find_map(|file| {
                        let state = TaskState::load(&file.dir, &file.id).ok()?;
                        let task = state
                            .session_tasks
                            .iter()
                            .find(|task| task.agent_id == selector)?
                            .clone();
                        Some((state, task))
                    });
                if let Some((state, task)) = by_agent {
                    self.push_history_line(format!(
                        "[watch] {} parent={} agent={} status={} worktree={}",
                        task.id,
                        state.id,
                        task.agent_id,
                        task.lifecycle_state,
                        task.worktree_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "shared".to_string()),
                    ));
                } else {
                    self.push_history_line(format!(
                        "[watch] no session task found for '{selector}'"
                    ));
                }
            }
            Err(_) => {
                self.push_history_line(format!("[watch] error looking up '{selector}'"));
            }
        }
    }
}
