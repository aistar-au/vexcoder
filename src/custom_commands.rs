use crate::workspace::workspace_root;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const PROJECT_COMMANDS_REL_PATH: &str = ".vex/commands";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CustomCommand {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) template: String,
}

impl CustomCommand {
    pub(crate) fn display(&self) -> String {
        format!("/{} [input]", self.name)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CustomCommandFile {
    name: String,
    description: String,
    template: String,
}

pub(crate) fn load_custom_commands(
    working_dir: &Path,
    builtin_names: &[String],
) -> Vec<CustomCommand> {
    let builtin_names = builtin_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut commands = BTreeMap::new();

    if let Some(user_dir) = user_commands_dir() {
        load_commands_from_dir(&user_dir, &builtin_names, &mut commands);
    }

    let project_dir = workspace_root(working_dir).join(PROJECT_COMMANDS_REL_PATH);
    load_commands_from_dir(&project_dir, &builtin_names, &mut commands);

    commands.into_values().collect()
}

fn load_commands_from_dir(
    dir: &Path,
    builtin_names: &BTreeSet<String>,
    commands: &mut BTreeMap<String, CustomCommand>,
) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };

    let mut entries = read_dir
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    entries.sort();

    for path in entries {
        let Some(command) = load_command_file(&path, builtin_names) else {
            continue;
        };
        commands.insert(command.name.clone(), command);
    }
}

fn load_command_file(path: &Path, builtin_names: &BTreeSet<String>) -> Option<CustomCommand> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!(
                "[commands] skipping {}: failed to read: {error}",
                path.display()
            );
            return None;
        }
    };

    let command: CustomCommandFile = match toml::from_str(&raw) {
        Ok(command) => command,
        Err(error) => {
            eprintln!(
                "[commands] skipping {}: unknown or invalid key: {error}",
                path.display()
            );
            return None;
        }
    };

    let name = command.name.trim().to_ascii_lowercase();
    if !is_valid_command_name(&name) {
        eprintln!(
            "[commands] skipping {}: invalid command name '{}'",
            path.display(),
            command.name
        );
        return None;
    }
    if name.starts_with("vex-") {
        eprintln!(
            "[commands] skipping {}: command names beginning with 'vex-' are reserved",
            path.display()
        );
        return None;
    }
    if builtin_names.contains(&name) {
        eprintln!(
            "[commands] skipping {}: '{}' shadows a built-in command",
            path.display(),
            command.name
        );
        return None;
    }

    let description = command.description.trim().to_string();
    if description.is_empty() {
        eprintln!(
            "[commands] skipping {}: description must not be empty",
            path.display()
        );
        return None;
    }

    let template = command.template.trim().to_string();
    if template.is_empty() {
        eprintln!(
            "[commands] skipping {}: template must not be empty",
            path.display()
        );
        return None;
    }

    Some(CustomCommand {
        name,
        description,
        template,
    })
}

fn is_valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn user_commands_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("vex").join("commands"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ENV_LOCK;

    #[test]
    fn test_custom_command_display() {
        let command = CustomCommand {
            name: "standup".to_string(),
            description: "desc".to_string(),
            template: "tmpl".to_string(),
        };
        assert_eq!(command.display(), "/standup [input]");
    }

    #[tokio::test]
    async fn test_project_command_overrides_user_command() {
        let _env_lock = ENV_LOCK.lock().await;
        let project = tempfile::tempdir().unwrap();
        let xdg = tempfile::tempdir().unwrap();
        crate::test_support::test_set_var(&_env_lock, "XDG_CONFIG_HOME", xdg.path());

        let project_commands = project.path().join(".vex/commands");
        let user_commands = xdg.path().join("vex/commands");
        std::fs::create_dir_all(&project_commands).unwrap();
        std::fs::create_dir_all(&user_commands).unwrap();
        std::fs::write(
            user_commands.join("standup.toml"),
            "name = \"standup\"\ndescription = \"user\"\ntemplate = \"user\"\n",
        )
        .unwrap();
        std::fs::write(
            project_commands.join("standup.toml"),
            "name = \"standup\"\ndescription = \"project\"\ntemplate = \"project\"\n",
        )
        .unwrap();

        let commands = load_custom_commands(project.path(), &[]);
        crate::test_support::test_remove_var(&_env_lock, "XDG_CONFIG_HOME");

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].description, "project");
    }

    #[tokio::test]
    async fn test_builtin_shadow_is_skipped() {
        let _env_lock = ENV_LOCK.lock().await;
        let project = tempfile::tempdir().unwrap();
        let project_commands = project.path().join(".vex/commands");
        std::fs::create_dir_all(&project_commands).unwrap();
        std::fs::write(
            project_commands.join("edit.toml"),
            "name = \"edit\"\ndescription = \"shadow\"\ntemplate = \"shadow\"\n",
        )
        .unwrap();

        let commands = load_custom_commands(project.path(), &["edit".to_string()]);
        assert!(commands.is_empty());
    }
}
