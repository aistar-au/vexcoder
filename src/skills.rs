use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

const REGISTRY_REL_PATH: &str = ".agents/skills/registry.toml";
const UNKNOWN_SKILL_VERSION: &str = "unknown";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallSourceKind {
    GitRepo,
    Tarball,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillEntry {
    pub name: String,
    pub version: String,
    pub source: String,
    pub path: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SkillsRegistry {
    #[serde(default)]
    pub skills: Vec<SkillEntry>,
    #[serde(skip)]
    registry_path: Option<PathBuf>,
    #[serde(skip)]
    workspace_root: Option<PathBuf>,
}

impl SkillsRegistry {
    pub fn load(working_dir: &Path) -> Result<Self> {
        let workspace_root = crate::workspace::workspace_root(working_dir);
        let path = workspace_root.join(REGISTRY_REL_PATH);
        let mut registry: Self = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            toml::from_str(&text)?
        } else {
            Self::default()
        };
        registry.registry_path = Some(path);
        registry.workspace_root = Some(workspace_root);
        Ok(registry)
    }

    fn save(&self) -> Result<()> {
        let path = self
            .registry_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("registry path not set"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    fn workspace_root(&self) -> Result<&Path> {
        self.workspace_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("workspace root not set"))
    }

    pub fn get(&self, name: &str) -> Option<&SkillEntry> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    fn detect_source_kind(source: &str) -> Result<InstallSourceKind> {
        if source.contains("raw.githubusercontent.com")
            || source.contains("/raw/")
            || source.contains("/blob/")
        {
            bail!(
                "raw or blob URL fetch is not supported; use a git repository URL or a tarball URL (.tar.gz / .tgz / .zip)"
            );
        }
        if !source.starts_with("http://") && !source.starts_with("https://") {
            bail!("source must be an http:// or https:// URL");
        }

        let trimmed = strip_query_fragment(source);
        if trimmed.ends_with(".tar.gz") || trimmed.ends_with(".tgz") || trimmed.ends_with(".zip") {
            Ok(InstallSourceKind::Tarball)
        } else {
            Ok(InstallSourceKind::GitRepo)
        }
    }

    pub fn validate_source(source: &str) -> Result<()> {
        Self::detect_source_kind(source).map(|_| ())
    }

    pub fn install(&mut self, source: &str, subdir: Option<&str>) -> Result<String> {
        let source_kind = Self::detect_source_kind(source)?;
        let staging = tempfile::tempdir()?;
        let fetched_root = fetch_source(source, source_kind, staging.path())?;
        let skill_root = select_skill_root(source_kind, &fetched_root, subdir)?;
        if !skill_root.join("SKILL.md").exists() {
            bail!("skill at {source} does not contain a SKILL.md entrypoint");
        }
        let name = derive_skill_name(source_kind, source, subdir, &skill_root)?;

        let dest = self.workspace_root()?.join(".agents/skills").join(&name);
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        std::fs::create_dir_all(&dest)?;
        copy_dir_contents(&skill_root, &dest)?;

        let entry = SkillEntry {
            name: name.clone(),
            version: UNKNOWN_SKILL_VERSION.to_string(),
            source: source.to_string(),
            path: format!(".agents/skills/{name}"),
        };
        upsert_entry(&mut self.skills, entry);
        self.save()?;
        println!("installed skill '{name}' from {source}");
        Ok(name)
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        let entry = self
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("skill '{name}' not found in registry"))?;
        let skill_dir = self.workspace_root()?.join(&entry.path);
        if skill_dir.exists() {
            std::fs::remove_dir_all(&skill_dir)?;
        }
        self.skills.retain(|skill| skill.name != name);
        self.save()?;
        println!("removed skill '{name}'");
        Ok(())
    }

    pub fn list(&self) {
        if self.skills.is_empty() {
            println!("no skills installed");
            return;
        }

        let mut names = self
            .skills
            .iter()
            .map(|skill| format!("{} ({})", skill.name, skill.version))
            .collect::<Vec<_>>();
        names.sort();
        for line in names {
            println!("{line}");
        }
    }
}

fn strip_query_fragment(source: &str) -> &str {
    source.split(['?', '#']).next().unwrap_or(source)
}

fn source_basename(source: &str) -> Option<String> {
    let trimmed = strip_query_fragment(source).trim_end_matches('/');
    let segment = trimmed.rsplit('/').next()?;
    let segment = segment
        .strip_suffix(".tar.gz")
        .or_else(|| segment.strip_suffix(".tgz"))
        .or_else(|| segment.strip_suffix(".zip"))
        .or_else(|| segment.strip_suffix(".git"))
        .unwrap_or(segment);
    if segment.is_empty() {
        None
    } else {
        Some(segment.to_string())
    }
}

fn derive_skill_name(
    source_kind: InstallSourceKind,
    source: &str,
    subdir: Option<&str>,
    skill_root: &Path,
) -> Result<String> {
    if let Some(subdir) = subdir {
        return Path::new(subdir)
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .ok_or_else(|| anyhow::anyhow!("--subdir must end with a skill directory name"));
    }

    if source_kind == InstallSourceKind::Tarball
        && let Some(name) = skill_root.file_name().and_then(|name| name.to_str())
    {
        return Ok(name.to_string());
    }

    source_basename(source)
        .ok_or_else(|| anyhow::anyhow!("could not derive a skill name from {source}"))
}

fn fetch_source(source: &str, source_kind: InstallSourceKind, staging: &Path) -> Result<PathBuf> {
    match source_kind {
        InstallSourceKind::GitRepo => {
            let clone_dir = staging.join("clone");
            let status = Command::new("git")
                .args(["clone", "--depth=1", source])
                .arg(&clone_dir)
                .status()
                .with_context(|| format!("failed to run git clone for {source}"))?;
            if !status.success() {
                bail!("git clone failed for {source}");
            }
            Ok(clone_dir)
        }
        InstallSourceKind::Tarball => {
            let archive_name = source_basename(source).unwrap_or_else(|| "skill".to_string());
            let archive_path = staging.join(archive_name);
            let extract_dir = staging.join("extracted");
            std::fs::create_dir_all(&extract_dir)?;

            let status = Command::new("curl")
                .args(["-sSfL", source, "-o"])
                .arg(&archive_path)
                .status()
                .with_context(|| format!("failed to download {source}"))?;
            if !status.success() {
                bail!("curl download failed for {source}");
            }

            if strip_query_fragment(source).ends_with(".zip") {
                let status = Command::new("unzip")
                    .args(["-q"])
                    .arg(&archive_path)
                    .args(["-d"])
                    .arg(&extract_dir)
                    .status()
                    .with_context(|| format!("failed to unzip {source}"))?;
                if !status.success() {
                    bail!("unzip failed for {source}");
                }
            } else {
                let status = Command::new("tar")
                    .args(["xzf"])
                    .arg(&archive_path)
                    .args(["-C"])
                    .arg(&extract_dir)
                    .status()
                    .with_context(|| format!("failed to unpack {source}"))?;
                if !status.success() {
                    bail!("tar extraction failed for {source}");
                }
            }

            Ok(extract_dir)
        }
    }
}

fn select_skill_root(
    source_kind: InstallSourceKind,
    fetched_root: &Path,
    subdir: Option<&str>,
) -> Result<PathBuf> {
    if let Some(subdir) = subdir {
        let root = fetched_root.join(subdir);
        if !root.exists() {
            bail!("--subdir '{subdir}' not found in fetched source");
        }
        return Ok(root);
    }

    if fetched_root.join("SKILL.md").exists() {
        return Ok(fetched_root.to_path_buf());
    }

    if source_kind == InstallSourceKind::Tarball {
        let dirs = std::fs::read_dir(fetched_root)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(|kind| kind.is_dir())
                    .map(|_| entry)
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        if dirs.len() == 1 && dirs[0].join("SKILL.md").exists() {
            return Ok(dirs[0].clone());
        }
    }

    bail!(
        "fetched source does not expose a skill root at its top level; pass --subdir for nested skills"
    )
}

fn upsert_entry(skills: &mut Vec<SkillEntry>, entry: SkillEntry) {
    if let Some(existing) = skills.iter_mut().find(|skill| skill.name == entry.name) {
        *existing = entry;
    } else {
        skills.push(entry);
        skills.sort_by(|left, right| left.name.cmp(&right.name));
    }
}

fn copy_dir_contents(src: &Path, dest: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_name = entry.file_name();
        if file_name == ".git" {
            continue;
        }

        let target = dest.join(&file_name);
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_dir_contents(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry(root: &Path, toml_str: &str) -> SkillsRegistry {
        let path = root.join(".agents/skills/registry.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        if !toml_str.is_empty() {
            std::fs::write(&path, toml_str).unwrap();
        }
        SkillsRegistry::load(root).unwrap()
    }

    #[test]
    fn test_skills_registry_load_uses_repo_root_from_subdir() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();
        let path = temp.path().join(".agents/skills/registry.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "[[skills]]\nname = \"edit-loop\"\nversion = \"1.0.0\"\nsource = \"local\"\npath = \".agents/skills/edit-loop\"\n",
        )
        .unwrap();

        let registry = SkillsRegistry::load(&nested).unwrap();
        assert!(registry.get("edit-loop").is_some());
        assert_eq!(registry.registry_path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn test_skills_install_rejects_raw_or_blob_url() {
        assert!(
            SkillsRegistry::validate_source(
                "https://raw.githubusercontent.com/x/y/main/skills/edit-loop/SKILL.md"
            )
            .is_err()
        );
        assert!(
            SkillsRegistry::validate_source("https://github.com/x/y/blob/main/SKILL.md").is_err()
        );
    }

    #[test]
    fn test_skills_install_rejects_non_http_source() {
        assert!(SkillsRegistry::validate_source("file:///tmp/skill").is_err());
    }

    #[test]
    fn test_skills_install_accepts_git_and_tarball_sources() {
        assert!(SkillsRegistry::validate_source("https://github.com/example/skills.git").is_ok());
        assert!(SkillsRegistry::validate_source("https://example.com/edit-loop.tar.gz").is_ok());
    }

    #[test]
    fn test_derive_skill_name_from_subdir() {
        let root = Path::new("/tmp/skill/edit-loop");
        let name = derive_skill_name(
            InstallSourceKind::GitRepo,
            "https://github.com/example/skills.git",
            Some("skills/edit-loop"),
            root,
        )
        .unwrap();
        assert_eq!(name, "edit-loop");
    }

    #[test]
    fn test_copy_dir_contents_skips_git_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("src");
        let dest = temp.path().join("dest");
        std::fs::create_dir_all(src.join(".git")).unwrap();
        std::fs::write(src.join(".git/config"), "[core]\n").unwrap();
        std::fs::write(src.join("SKILL.md"), "# skill\n").unwrap();

        std::fs::create_dir_all(&dest).unwrap();
        copy_dir_contents(&src, &dest).unwrap();

        assert!(dest.join("SKILL.md").exists());
        assert!(!dest.join(".git").exists());
    }

    #[test]
    fn test_remove_uses_registered_path() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let mut registry = make_registry(
            temp.path(),
            "[[skills]]\nname = \"edit-loop\"\nversion = \"1.0.0\"\nsource = \"local\"\npath = \".agents/skills/edit-loop\"\n",
        );
        let skill_dir = temp.path().join(".agents/skills/edit-loop");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# skill\n").unwrap();

        registry.remove("edit-loop").unwrap();

        assert!(registry.get("edit-loop").is_none());
        assert!(!skill_dir.exists());
    }
}
