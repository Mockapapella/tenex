//! Docker runtime support for agent processes.

use crate::agent::Agent;
use crate::app::Settings;
use crate::paths;
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(test)]
use std::sync::{Mutex, OnceLock, RwLock};

const DEFAULT_DOCKER_IMAGE: &str = "tenex-worker:latest";
const DEFAULT_WORKER_DOCKERFILE_TEMPLATE: &str = include_str!("../../docker/worker.Dockerfile");
const RUNTIME_HOME_ROOT_DIR: &str = "docker-runtime";
const WORKER_IMAGE_TEMPLATE_HASH_LABEL: &str = "dev.tenex.worker-template-fnv1a64";
const WORKER_CONTAINER_LAYOUT_HASH_LABEL: &str = "dev.tenex.runtime-template-fnv1a64";
const WORKER_CONTAINER_LAYOUT_VERSION: &str = "3";
#[cfg(any(test, windows))]
const WINDOWS_CONTAINER_ROOT: &str = "/tenex-host";

struct PreparedRuntimeHome {
    home_source: PathBuf,
    codex_home_source: PathBuf,
    codex_home_target: PathBuf,
}

pub(super) fn wrap_exec(agent: &Agent, _settings: &Settings, command: &[String]) -> Vec<String> {
    let mut argv = exec_prefix(agent);
    argv.push(container_name(agent));
    argv.extend(command.iter().cloned());
    argv
}

pub(super) fn check_available() -> Result<()> {
    let mut cmd = docker_command();
    cmd.arg("version");

    match cmd.output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                "the Docker daemon may be unavailable".to_string()
            };
            bail!("Docker is unavailable: `docker version` failed: {detail}");
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            bail!("Docker is not installed or not on PATH");
        }
        Err(err) => bail!("Failed to run `docker version`: {err}"),
    }
}

pub(super) fn ensure_image_ready(settings: &Settings, program: &str) -> Result<()> {
    let image = worker_image_tag(settings);
    ensure_default_image_support(program)?;
    if image_matches_default_template(image)? {
        return Ok(());
    }

    build_default_image(image)
}

pub(super) fn image_build_required(settings: &Settings, program: &str) -> Result<bool> {
    ensure_default_image_support(program)?;
    Ok(!image_matches_default_template(worker_image_tag(settings))?)
}

pub(super) fn ensure_container(agent: &Agent, settings: &Settings) -> Result<()> {
    let home = paths::home_dir();
    let data_local_dir = paths::data_local_dir();
    let ssh_auth_sock = std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from);
    ensure_container_with_paths(
        agent,
        settings,
        home.as_deref(),
        data_local_dir.as_deref(),
        ssh_auth_sock.as_deref(),
    )
}

fn ensure_container_with_paths(
    agent: &Agent,
    settings: &Settings,
    home: Option<&Path>,
    data_local_dir: Option<&Path>,
    ssh_auth_sock: Option<&Path>,
) -> Result<()> {
    check_available()?;
    ensure_image_ready(settings, &agent.program)?;
    if let Some(home) = home {
        prepare_runtime_home_with_data_local_dir(agent, home, data_local_dir)?;
    }
    let name = container_name(agent);
    let inspect = run_command(
        docker_command().args(["inspect", "--format", "{{.State.Running}}", &name]),
        "Failed to inspect Docker container",
    );

    match inspect {
        Ok(output) => {
            let is_running = output.trim() == "true";
            if !container_matches_current_layout(&name)? {
                if is_running {
                    bail!(
                        "Docker container `{name}` uses an older Tenex worker layout. Remove the root agent or run `docker rm -f {name}` so Tenex can recreate it."
                    );
                }
                remove_container_by_name(&name)?;
            } else if is_running {
                return Ok(());
            } else {
                run_command(
                    docker_command().args(["start", &name]),
                    "Failed to start Docker container",
                )?;
                return Ok(());
            }
        }
        Err(err) => {
            let message = format!("{err:#}");
            if !message.contains("No such object") && !message.contains("No such container") {
                return Err(err);
            }
        }
    }

    let image = worker_image_tag(settings);
    let worktree = &agent.worktree_path;
    let worktree_target = container_target_path(worktree);
    let mut cmd = docker_command();
    cmd.args(["run", "-d", "--init", "--name", &name, "--hostname", &name]);
    cmd.arg("--label").arg(format!(
        "{WORKER_CONTAINER_LAYOUT_HASH_LABEL}={}",
        current_container_layout_hash()
    ));

    if let Some(user) = docker_user_arg() {
        cmd.args(["--user", &user]);
    }

    if let Some(home) = home {
        configure_home_mounts_with_data_local_dir(&mut cmd, agent, home, data_local_dir)?;
    }

    configure_ssh_auth_sock_mount_from(&mut cmd, ssh_auth_sock);
    configure_repo_metadata_mounts(&mut cmd, agent);

    cmd.args(["-w", &display_path(&worktree_target)]);
    add_bind_mount(&mut cmd, worktree, &worktree_target, false);

    cmd.arg(image);
    cmd.arg("sleep");
    cmd.arg("infinity");

    run_command(&mut cmd, "Failed to create Docker container")?;
    Ok(())
}

pub(super) fn remove_container(agent: &Agent) -> Result<()> {
    remove_container_by_name(&container_name(agent))
}

fn remove_container_by_name(name: &str) -> Result<()> {
    match run_command(
        docker_command().args(["rm", "-f", name]),
        "Failed to remove Docker container",
    ) {
        Ok(_) => Ok(()),
        Err(err) => {
            let message = format!("{err:#}");
            if message.contains("No such container") || message.contains("No such object") {
                return Ok(());
            }
            Err(err)
        }
    }
}

fn exec_prefix(agent: &Agent) -> Vec<String> {
    let forwarded_env = collect_forwarded_exec_env(|key| std::env::var(key).ok());
    exec_prefix_with_forwarded_env(agent, &forwarded_env)
}

fn exec_prefix_with_forwarded_env(agent: &Agent, forwarded_env: &[(&str, String)]) -> Vec<String> {
    let worktree_target = container_target_path(&agent.worktree_path);
    let mut argv = vec![
        "docker".to_string(),
        "exec".to_string(),
        "-it".to_string(),
        "-w".to_string(),
        display_path(&worktree_target),
    ];

    if let Some(home) = paths::home_dir() {
        let home_target = container_target_path(&home);
        let codex_home_target = container_target_path(&codex_home_dir(&home));
        argv.push("-e".to_string());
        argv.push(format!("HOME={}", home_target.display()));
        argv.push("-e".to_string());
        argv.push(format!(
            "XDG_CACHE_HOME={}",
            home_target.join(".cache").display()
        ));
        argv.push("-e".to_string());
        argv.push(format!(
            "CARGO_HOME={}",
            home_target.join(".cargo").display()
        ));
        argv.push("-e".to_string());
        argv.push(format!("CODEX_HOME={}", codex_home_target.display()));
    }

    for (key, value) in forwarded_env {
        argv.push("-e".to_string());
        argv.push(format!("{key}={value}"));
    }

    argv
}

fn collect_forwarded_exec_env(
    mut get: impl FnMut(&str) -> Option<String>,
) -> Vec<(&'static str, String)> {
    ["TERM", "COLORTERM", "SSH_AUTH_SOCK"]
        .into_iter()
        .filter_map(|key| {
            let value = get(key)?;
            if value.trim().is_empty() {
                return None;
            }
            Some((key, forwarded_env_value(key, &value)))
        })
        .collect()
}

const fn worker_image_tag(_settings: &Settings) -> &str {
    DEFAULT_DOCKER_IMAGE
}

pub(super) fn container_name(agent: &Agent) -> String {
    let mut name = format!("tenex-runtime-{}", agent.effective_runtime_scope());
    name.make_ascii_lowercase();
    name.chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' | '.' | '_' | '-' => ch,
            _ => '-',
        })
        .collect()
}

fn add_optional_bind_mount(cmd: &mut Command, source: &Path, target: &Path, readonly: bool) {
    if source.exists() {
        add_bind_mount(cmd, source, target, readonly);
    }
}

fn add_bind_mount(cmd: &mut Command, source: &Path, target: &Path, readonly: bool) {
    let mut spec = format!("{}:{}", source.display(), target.display());
    if readonly {
        spec.push_str(":ro");
    }
    cmd.arg("-v").arg(spec);
}

fn display_path(path: &Path) -> String {
    path.as_os_str().to_string_lossy().into_owned()
}

pub(super) fn session_workdir(agent: &Agent) -> PathBuf {
    container_target_path(&agent.worktree_path)
}

fn container_target_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        windows_container_target_from_str(&path.to_string_lossy())
    }

    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

#[cfg(any(test, windows))]
fn windows_container_target_from_str(path: &str) -> PathBuf {
    let normalized = path.replace('\\', "/");
    let bytes = normalized.as_bytes();

    if bytes.len() >= 3
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
        && (bytes[0] as char).is_ascii_alphabetic()
    {
        let mut target = PathBuf::from(WINDOWS_CONTAINER_ROOT);
        target.push(((bytes[0] as char).to_ascii_lowercase()).to_string());
        for segment in normalized[3..]
            .split('/')
            .filter(|segment| !segment.is_empty())
        {
            target.push(segment);
        }
        return target;
    }

    let mut target = PathBuf::from(WINDOWS_CONTAINER_ROOT);
    target.push("misc");
    for segment in normalized
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
    {
        target.push(segment.trim_end_matches(':'));
    }
    target
}

fn codex_home_dir(home: &Path) -> PathBuf {
    std::env::var_os("CODEX_HOME").map_or_else(|| home.join(".codex"), PathBuf::from)
}

fn configure_home_mounts_with_data_local_dir(
    cmd: &mut Command,
    agent: &Agent,
    home: &Path,
    data_local_dir: Option<&Path>,
) -> Result<()> {
    let prepared_home = prepare_runtime_home_with_data_local_dir(agent, home, data_local_dir)?;
    let home_target = container_target_path(home);
    let codex_home_target = container_target_path(&prepared_home.codex_home_target);
    cmd.arg("-e").arg(format!("HOME={}", home_target.display()));
    cmd.arg("-e").arg(format!(
        "XDG_CACHE_HOME={}",
        home_target.join(".cache").display()
    ));
    cmd.arg("-e").arg(format!(
        "CARGO_HOME={}",
        home_target.join(".cargo").display()
    ));
    cmd.arg("-e")
        .arg(format!("CODEX_HOME={}", codex_home_target.display()));

    add_bind_mount(cmd, &prepared_home.home_source, &home_target, false);
    add_bind_mount(
        cmd,
        &prepared_home.codex_home_source,
        &codex_home_target,
        false,
    );
    add_bind_mount(
        cmd,
        &prepared_home.codex_home_target.join("sessions"),
        &container_target_path(&prepared_home.codex_home_target.join("sessions")),
        false,
    );

    for name in ["skills", "prompts", "rules"] {
        add_optional_bind_mount(
            cmd,
            &prepared_home.codex_home_target.join(name),
            &container_target_path(&prepared_home.codex_home_target.join(name)),
            true,
        );
    }

    add_optional_bind_mount(
        cmd,
        &home.join(".config").join("gh"),
        &container_target_path(&home.join(".config").join("gh")),
        false,
    );
    add_optional_bind_mount(
        cmd,
        &home.join(".gitconfig"),
        &container_target_path(&home.join(".gitconfig")),
        true,
    );
    Ok(())
}

fn configure_ssh_auth_sock_mount_from(cmd: &mut Command, ssh_auth_sock: Option<&Path>) {
    if let Some(ssh_auth_sock) = ssh_auth_sock
        && ssh_auth_sock.exists()
    {
        cmd.arg("-e").arg(format!(
            "SSH_AUTH_SOCK={}",
            container_target_path(ssh_auth_sock).display()
        ));
        add_bind_mount(
            cmd,
            ssh_auth_sock,
            &container_target_path(ssh_auth_sock),
            false,
        );
    }
}

fn forwarded_env_value(key: &str, value: &str) -> String {
    if key == "SSH_AUTH_SOCK" {
        display_path(&container_target_path(Path::new(value)))
    } else {
        value.to_string()
    }
}

fn configure_repo_metadata_mounts(cmd: &mut Command, agent: &Agent) {
    let mut mounted_targets = HashSet::new();
    if let Some(repo_root) = agent.repo_root.as_deref() {
        add_optional_bind_mount_once(
            cmd,
            &mut mounted_targets,
            &repo_root.join(".git"),
            &container_target_path(&repo_root.join(".git")),
            false,
        );
        add_optional_bind_mount_once(
            cmd,
            &mut mounted_targets,
            &repo_root.join("worktrees"),
            &container_target_path(&repo_root.join("worktrees")),
            false,
        );
    }
    configure_top_level_symlink_mounts(cmd, &agent.worktree_path, &mut mounted_targets);
}

fn add_optional_bind_mount_once(
    cmd: &mut Command,
    mounted_targets: &mut HashSet<PathBuf>,
    source: &Path,
    target: &Path,
    readonly: bool,
) {
    if source.exists() && mounted_targets.insert(target.to_path_buf()) {
        add_bind_mount(cmd, source, target, readonly);
    }
}

fn configure_top_level_symlink_mounts(
    cmd: &mut Command,
    worktree: &Path,
    mounted_targets: &mut HashSet<PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(worktree) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_symlink() {
            continue;
        }

        let Ok(link_target) = std::fs::read_link(&path) else {
            continue;
        };
        let resolved = resolved_symlink_target(&path, worktree, &link_target);
        if resolved.starts_with(worktree) {
            continue;
        }

        add_optional_bind_mount_once(
            cmd,
            mounted_targets,
            &resolved,
            &container_target_path(&resolved),
            false,
        );
    }
}

fn resolved_symlink_target(path: &Path, worktree: &Path, link_target: &Path) -> PathBuf {
    let resolved = if link_target.is_absolute() {
        link_target.to_path_buf()
    } else {
        path.parent().unwrap_or(worktree).join(link_target)
    };
    resolved.canonicalize().unwrap_or(resolved)
}

fn prepare_runtime_home_with_data_local_dir(
    agent: &Agent,
    home: &Path,
    data_local_dir: Option<&Path>,
) -> Result<PreparedRuntimeHome> {
    let data_local_dir = data_local_dir
        .map(Path::to_path_buf)
        .or_else(paths::data_local_dir)
        .or_else(paths::home_dir)
        .unwrap_or_else(std::env::temp_dir);
    let codex_home_target = codex_home_dir(home);
    prepare_runtime_home_in(agent, home, &data_local_dir, &codex_home_target)
}

fn prepare_runtime_home_in(
    agent: &Agent,
    home: &Path,
    data_local_dir: &Path,
    codex_home_target: &Path,
) -> Result<PreparedRuntimeHome> {
    let runtime_root = runtime_root_dir(agent, data_local_dir);
    let home_source = runtime_root.join("home");
    let codex_home_source = runtime_root.join("codex-home");

    std::fs::create_dir_all(home_source.join(".cache")).with_context(|| {
        format!(
            "Failed to create Docker runtime cache directory {}",
            home_source.join(".cache").display()
        )
    })?;
    std::fs::create_dir_all(home_source.join(".cargo")).with_context(|| {
        format!(
            "Failed to create Docker runtime Cargo directory {}",
            home_source.join(".cargo").display()
        )
    })?;
    std::fs::create_dir_all(home_source.join(".config")).with_context(|| {
        format!(
            "Failed to create Docker runtime config directory {}",
            home_source.join(".config").display()
        )
    })?;
    std::fs::create_dir_all(home_source.join(".local").join("share")).with_context(|| {
        format!(
            "Failed to create Docker runtime local share directory {}",
            home_source.join(".local").join("share").display()
        )
    })?;
    std::fs::create_dir_all(codex_home_target.join("sessions")).with_context(|| {
        format!(
            "Failed to create host Codex sessions directory {}",
            codex_home_target.join("sessions").display()
        )
    })?;
    sync_claude_home(&home_source, home)?;
    sync_codex_home(&codex_home_source, codex_home_target)?;

    Ok(PreparedRuntimeHome {
        home_source,
        codex_home_source,
        codex_home_target: codex_home_target.to_path_buf(),
    })
}

fn runtime_root_dir(agent: &Agent, data_local_dir: &Path) -> PathBuf {
    data_local_dir
        .join("tenex")
        .join(RUNTIME_HOME_ROOT_DIR)
        .join(container_name(agent))
}

fn sync_codex_home(target: &Path, host_codex_home: &Path) -> Result<()> {
    std::fs::create_dir_all(target).with_context(|| {
        format!(
            "Failed to create managed Codex home directory {}",
            target.display()
        )
    })?;

    let config_source = host_codex_home.join("config.toml");
    let config_target = target.join("config.toml");
    if config_source.is_file() {
        let config = std::fs::read_to_string(&config_source)
            .with_context(|| format!("Failed to read {}", config_source.display()))?;
        std::fs::write(&config_target, sanitize_codex_config(&config))
            .with_context(|| format!("Failed to write {}", config_target.display()))?;
    } else if config_target.exists() {
        std::fs::remove_file(&config_target)
            .with_context(|| format!("Failed to remove {}", config_target.display()))?;
    }

    for file_name in ["auth.json", "version.json", ".personality_migration"] {
        sync_optional_file(&host_codex_home.join(file_name), &target.join(file_name))?;
    }

    Ok(())
}

fn sync_optional_file(source: &Path, target: &Path) -> Result<()> {
    if source.is_file() {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::copy(source, target).with_context(|| {
            format!(
                "Failed to copy {} to {}",
                source.display(),
                target.display()
            )
        })?;
    } else if target.exists() {
        std::fs::remove_file(target)
            .with_context(|| format!("Failed to remove {}", target.display()))?;
    }

    Ok(())
}

fn sync_optional_dir(source: &Path, target: &Path) -> Result<()> {
    if source.is_dir() {
        if target.exists() {
            std::fs::remove_dir_all(target)
                .with_context(|| format!("Failed to remove {}", target.display()))?;
        }
        copy_dir_recursive(source, target)?;
    } else if target.exists() {
        std::fs::remove_dir_all(target)
            .with_context(|| format!("Failed to remove {}", target.display()))?;
    }

    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target)
        .with_context(|| format!("Failed to create {}", target.display()))?;

    for entry in
        std::fs::read_dir(source).with_context(|| format!("Failed to read {}", source.display()))?
    {
        let entry = entry.with_context(|| format!("Failed to read {}", source.display()))?;
        let entry_type = entry
            .file_type()
            .with_context(|| format!("Failed to read file type for {}", entry.path().display()))?;
        let target_path = target.join(entry.file_name());
        if entry_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target_path)?;
        } else if entry_type.is_file() {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            std::fs::copy(entry.path(), &target_path).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    entry.path().display(),
                    target_path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn sync_claude_home(target_home: &Path, host_home: &Path) -> Result<()> {
    let host_claude_dir = host_home.join(".claude");
    let target_claude_dir = target_home.join(".claude");
    std::fs::create_dir_all(&target_claude_dir)
        .with_context(|| format!("Failed to create {}", target_claude_dir.display()))?;

    sync_optional_file(
        &host_home.join(".claude.json"),
        &target_home.join(".claude.json"),
    )?;
    sync_optional_file(
        &host_claude_dir.join(".credentials.json"),
        &target_claude_dir.join(".credentials.json"),
    )?;
    sync_optional_file(
        &host_claude_dir.join("mcp-needs-auth-cache.json"),
        &target_claude_dir.join("mcp-needs-auth-cache.json"),
    )?;
    sync_claude_settings_file(
        &host_claude_dir.join("settings.json"),
        &target_claude_dir.join("settings.json"),
    )?;
    sync_claude_settings_file(
        &host_claude_dir.join("settings.local.json"),
        &target_claude_dir.join("settings.local.json"),
    )?;

    for dir_name in ["agents", "commands", "output-styles", "skills"] {
        sync_optional_dir(
            &host_claude_dir.join(dir_name),
            &target_claude_dir.join(dir_name),
        )?;
    }

    Ok(())
}

fn sync_claude_settings_file(source: &Path, target: &Path) -> Result<()> {
    if source.is_file() {
        let contents = std::fs::read_to_string(source)
            .with_context(|| format!("Failed to read {}", source.display()))?;
        std::fs::write(target, sanitize_claude_settings(&contents))
            .with_context(|| format!("Failed to write {}", target.display()))?;
    } else if target.exists() {
        std::fs::remove_file(target)
            .with_context(|| format!("Failed to remove {}", target.display()))?;
    }

    Ok(())
}

fn sanitize_codex_config(contents: &str) -> String {
    let mut sanitized = String::new();
    let mut skipping_mcp_section = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        let is_table = trimmed.starts_with('[') && trimmed.ends_with(']');
        if is_table {
            skipping_mcp_section =
                trimmed == "[mcp_servers]" || trimmed.starts_with("[mcp_servers.");
        }

        if skipping_mcp_section || trimmed.starts_with("notify =") {
            continue;
        }

        sanitized.push_str(line);
        sanitized.push('\n');
    }

    sanitized
}

fn sanitize_claude_settings(contents: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(contents) else {
        return contents.to_string();
    };

    if let Some(object) = value.as_object_mut() {
        object.remove("hooks");
    }

    serde_json::to_string_pretty(&value).unwrap_or_else(|_| contents.to_string())
}

fn docker_command() -> Command {
    Command::new(docker_program())
}

fn docker_program() -> PathBuf {
    #[cfg(test)]
    {
        let override_path = docker_program_override_store()
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(path) = override_path {
            return path;
        }
    }

    PathBuf::from("docker")
}

fn image_matches_default_template(image: &str) -> Result<bool> {
    let Some(actual_hash) = image_template_hash(image)? else {
        return Ok(false);
    };
    Ok(actual_hash == default_worker_dockerfile_hash())
}

fn image_template_hash(image: &str) -> Result<Option<String>> {
    match run_command(
        docker_command().args([
            "image",
            "inspect",
            "--format",
            &format!("{{{{ index .Config.Labels \"{WORKER_IMAGE_TEMPLATE_HASH_LABEL}\" }}}}"),
            image,
        ]),
        "Failed to inspect Docker image",
    ) {
        Ok(output) => {
            let hash = output.trim();
            if hash.is_empty() || hash == "<no value>" {
                return Ok(None);
            }
            Ok(Some(hash.to_string()))
        }
        Err(err) => {
            let message = format!("{err:#}");
            if message.contains("No such image") || message.contains("No such object") {
                return Ok(None);
            }
            Err(err)
        }
    }
}

fn container_matches_current_layout(name: &str) -> Result<bool> {
    let Some(actual_hash) = container_layout_hash(name)? else {
        return Ok(false);
    };
    Ok(actual_hash == current_container_layout_hash())
}

fn container_layout_hash(name: &str) -> Result<Option<String>> {
    match run_command(
        docker_command().args([
            "inspect",
            "--format",
            &format!("{{{{ index .Config.Labels \"{WORKER_CONTAINER_LAYOUT_HASH_LABEL}\" }}}}"),
            name,
        ]),
        "Failed to inspect Docker container",
    ) {
        Ok(output) => {
            let hash = output.trim();
            if hash.is_empty() || hash == "<no value>" {
                return Ok(None);
            }
            Ok(Some(hash.to_string()))
        }
        Err(err) => {
            let message = format!("{err:#}");
            if message.contains("No such container") || message.contains("No such object") {
                return Ok(None);
            }
            Err(err)
        }
    }
}

fn ensure_default_image_support(program: &str) -> Result<()> {
    match crate::conversation::detect_agent_cli(program) {
        crate::conversation::AgentCli::Claude | crate::conversation::AgentCli::Codex => Ok(()),
        crate::conversation::AgentCli::Other => bail!(
            "Docker mode only supports the built-in `claude` and `codex` CLIs. `{program}` is not supported by the shipped Tenex worker image"
        ),
    }
}

fn default_worker_dockerfile() -> String {
    DEFAULT_WORKER_DOCKERFILE_TEMPLATE.to_string()
}

fn default_worker_dockerfile_hash() -> String {
    format!("{:016x}", fnv1a64(default_worker_dockerfile().as_bytes()))
}

fn current_container_layout_hash() -> String {
    let descriptor = format!(
        "layout:{WORKER_CONTAINER_LAYOUT_VERSION};image:{};cargo-home:.cargo;mounts:repo-git,repo-worktrees,external-symlink-targets",
        default_worker_dockerfile_hash()
    );
    format!("{:016x}", fnv1a64(descriptor.as_bytes()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn default_worker_build_context_dir() -> Result<PathBuf> {
    let base = paths::data_local_dir()
        .or_else(paths::home_dir)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("tenex").join("docker-build-context");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create Docker build context at {}", dir.display()))?;
    Ok(dir)
}

fn build_default_image(image: &str) -> Result<()> {
    let dockerfile = default_worker_dockerfile();
    let context_dir = default_worker_build_context_dir()?;
    let mut cmd = docker_command();
    cmd.args([
        "build",
        "--tag",
        image,
        "--label",
        &format!(
            "{WORKER_IMAGE_TEMPLATE_HASH_LABEL}={}",
            default_worker_dockerfile_hash()
        ),
        "--file",
        "-",
        &display_path(&context_dir),
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    let program = cmd.get_program().to_string_lossy().into_owned();
    let args = cmd
        .get_args()
        .map(OsStr::to_string_lossy)
        .collect::<Vec<_>>()
        .join(" ");
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn Docker build: {program} {args}"))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(dockerfile.as_bytes())
            .context("Failed to write built-in Dockerfile to docker build")?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("Failed to wait for Docker build: {program} {args}"))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "Failed to build built-in Tenex worker image `{image}`: {program} {args} (stdout: {stdout}, stderr: {stderr})"
        );
    }

    Ok(())
}

fn run_command(cmd: &mut Command, context: &str) -> Result<String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let args = cmd
        .get_args()
        .map(OsStr::to_string_lossy)
        .collect::<Vec<_>>()
        .join(" ");
    let output = cmd
        .output()
        .with_context(|| format!("{context}: {program} {args}"))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("{context}: {program} {args} (stdout: {stdout}, stderr: {stderr})");
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(unix)]
fn docker_user_arg() -> Option<String> {
    let uid = read_trimmed_stdout("id", ["-u"])?;
    let gid = read_trimmed_stdout("id", ["-g"])?;
    Some(format!("{uid}:{gid}"))
}

#[cfg(not(unix))]
fn docker_user_arg() -> Option<String> {
    None
}

#[cfg(unix)]
fn read_trimmed_stdout<const N: usize>(program: &str, args: [&str; N]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
static DOCKER_TEST_SERIAL: Mutex<()> = Mutex::new(());
#[cfg(test)]
static DOCKER_PROGRAM_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

#[cfg(test)]
fn docker_program_override_store() -> &'static RwLock<Option<PathBuf>> {
    DOCKER_PROGRAM_OVERRIDE.get_or_init(|| RwLock::new(None))
}

#[cfg(test)]
struct DockerProgramOverrideGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for DockerProgramOverrideGuard {
    fn drop(&mut self) {
        *docker_program_override_store()
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

#[cfg(test)]
pub(super) fn with_docker_program_override_for_tests<T>(
    program: PathBuf,
    f: impl FnOnce() -> T,
) -> T {
    let _guard = DockerProgramOverrideGuard {
        _lock: DOCKER_TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    };
    *docker_program_override_store()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(program);
    f()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, AgentRuntime};
    use crate::app::Settings;
    use std::fs;
    use tempfile::TempDir;

    fn docker_agent() -> Agent {
        let mut agent = Agent::new(
            "Docker".to_string(),
            "codex".to_string(),
            "agent/docker".to_string(),
            PathBuf::from("/tmp/runtime-test"),
        );
        agent.runtime = AgentRuntime::Docker;
        agent.mux_session = "tenex-ABCD1234-root".to_string();
        agent
    }

    #[test]
    fn test_container_name_is_sanitized_and_lowercase() {
        let name = container_name(&docker_agent());
        assert_eq!(name, "tenex-runtime-tenex-abcd1234-root");
    }

    #[test]
    fn test_container_name_uses_stable_runtime_scope_when_present() {
        let mut agent = docker_agent();
        agent.runtime_scope = "root-1234567890abcdef".to_string();
        let original = container_name(&agent);
        agent.mux_session = "tenex-renamed-root".to_string();
        assert_eq!(container_name(&agent), original);
    }

    #[test]
    fn test_windows_container_target_from_str_maps_drive_paths() {
        let target = windows_container_target_from_str(r"C:\Users\quinten\.tenex\worktrees\repo");
        assert_eq!(
            target,
            PathBuf::from("/tenex-host/c/Users/quinten/.tenex/worktrees/repo")
        );
    }

    #[test]
    fn test_windows_container_target_from_str_falls_back_for_unc_like_paths() {
        let target = windows_container_target_from_str("//server/share/tenex");
        assert_eq!(target, PathBuf::from("/tenex-host/misc/server/share/tenex"));
    }

    #[test]
    fn test_session_workdir_uses_container_target_path() {
        let agent = docker_agent();
        assert_eq!(session_workdir(&agent), agent.worktree_path);
    }

    #[test]
    fn test_forwarded_env_value_maps_ssh_auth_sock_path() {
        let value = forwarded_env_value("SSH_AUTH_SOCK", "/tmp/ssh-agent.sock");
        assert_eq!(value, "/tmp/ssh-agent.sock");
        assert_eq!(
            forwarded_env_value("TERM", "xterm-256color"),
            "xterm-256color"
        );
    }

    #[test]
    fn test_exec_prefix_with_forwarded_env_adds_forwarded_values() {
        let argv = exec_prefix_with_forwarded_env(
            &docker_agent(),
            &[
                ("TERM", "xterm-256color".to_string()),
                ("COLORTERM", "truecolor".to_string()),
                ("SSH_AUTH_SOCK", "/tmp/ssh-agent.sock".to_string()),
            ],
        );

        assert!(argv.iter().any(|arg| arg == "TERM=xterm-256color"));
        assert!(argv.iter().any(|arg| arg == "COLORTERM=truecolor"));
        assert!(
            argv.iter()
                .any(|arg| arg == "SSH_AUTH_SOCK=/tmp/ssh-agent.sock")
        );
    }

    #[test]
    fn test_configure_ssh_auth_sock_mount_from_uses_container_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let ssh_auth_sock = temp.path().join("ssh-agent.sock");
        fs::write(&ssh_auth_sock, [])?;
        let mut cmd = Command::new("docker");

        configure_ssh_auth_sock_mount_from(&mut cmd, Some(&ssh_auth_sock));

        let args = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            args.iter()
                .any(|arg| { arg == &format!("SSH_AUTH_SOCK={}", ssh_auth_sock.display()) })
        );
        assert!(
            args.iter()
                .any(|arg| arg
                    == &format!("{}:{}", ssh_auth_sock.display(), ssh_auth_sock.display()))
        );
        Ok(())
    }

    #[cfg(unix)]
    fn write_fake_docker_script(temp: &TempDir, body: &str) -> Result<PathBuf> {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("docker");
        fs::write(&script, body)?;
        let mut perms = fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms)?;
        Ok(script)
    }

    #[test]
    fn test_check_available_reports_missing_docker_clearly() {
        let missing = PathBuf::from("/definitely/missing/tenex-docker");
        with_docker_program_override_for_tests(missing, || {
            let result = check_available();
            assert!(result.is_err());
            let err = result
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(err.contains("Docker is not installed or not on PATH"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_check_available_reports_docker_version_failure_clearly() -> Result<()> {
        let temp = TempDir::new()?;
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nif [ \"$1\" = \"version\" ]; then\n  echo 'Cannot connect to the Docker daemon' >&2\n  exit 1\nfi\nexit 0\n",
        )?;

        with_docker_program_override_for_tests(script, || {
            let result = check_available();
            assert!(result.is_err());
            let err = result
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(err.contains("Docker is unavailable: `docker version` failed"));
            assert!(err.contains("Cannot connect to the Docker daemon"));
        });
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_image_ready_builds_shipped_image_when_missing()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let log = temp.path().join("docker.log");
        let dockerfile = temp.path().join("Dockerfile");
        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  echo 'No such image' >&2\n  exit 1\nfi\nif [ \"$1\" = \"build\" ]; then\n  cat > \"{}\"\n  exit 0\nfi\nexit 0\n",
                log.display(),
                dockerfile.display(),
            ),
        )?;

        with_docker_program_override_for_tests(script, || {
            let result = ensure_image_ready(&Settings::default(), "codex");
            assert!(result.is_ok());
        });

        let log_contents = std::fs::read_to_string(&log)?;
        assert!(log_contents.contains("image inspect --format"));
        assert!(log_contents.contains(WORKER_IMAGE_TEMPLATE_HASH_LABEL));
        assert!(log_contents.contains("build --tag tenex-worker:latest --label"));
        assert!(log_contents.contains(WORKER_IMAGE_TEMPLATE_HASH_LABEL));
        assert!(log_contents.contains(&default_worker_dockerfile_hash()));
        assert!(log_contents.contains("--file -"));

        let dockerfile_contents = std::fs::read_to_string(&dockerfile)?;
        assert!(dockerfile_contents.contains("@openai/codex"));
        assert!(dockerfile_contents.contains("@anthropic-ai/claude-code"));
        assert!(dockerfile_contents.contains("rustup component add clippy llvm-tools rustfmt"));
        assert!(
            dockerfile_contents.contains("cargo install cargo-llvm-cov --locked --version 0.6.22")
        );
        assert!(dockerfile_contents.contains("/etc/profile.d/tenex-rust-path.sh"));
        assert!(dockerfile_contents.contains("/usr/local/cargo/bin/*"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_image_ready_rebuilds_stale_shipped_image()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let log = temp.path().join("docker.log");
        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' 'stale-hash'\n  exit 0\nfi\nif [ \"$1\" = \"build\" ]; then\n  cat >/dev/null\n  exit 0\nfi\nexit 0\n",
                log.display(),
            ),
        )?;

        with_docker_program_override_for_tests(script, || {
            let result = ensure_image_ready(&Settings::default(), "codex");
            assert!(result.is_ok());
        });

        let log_contents = std::fs::read_to_string(&log)?;
        assert!(log_contents.contains("image inspect --format"));
        assert!(log_contents.contains("build --tag tenex-worker:latest --label"));
        Ok(())
    }

    #[test]
    fn test_ensure_image_ready_rejects_custom_program_for_shipped_image() {
        let result = ensure_image_ready(&Settings::default(), "my-agent --flag");
        assert!(result.is_err());
        let err = result
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(err.contains("Docker mode only supports the built-in `claude` and `codex` CLIs"));
    }

    #[test]
    fn test_sanitize_codex_config_strips_notify_and_mcp_sections() {
        let config = r#"
model = "gpt-5.4"
notify = ["bash", "-lc", "$HOME/.local/bin/beep"]

[features]
rmcp_client = true

[mcp_servers.slack]
command = "docker"
args = ["run"]

[notice]
hide_rate_limit_model_nudge = true
"#;

        let sanitized = sanitize_codex_config(config);
        assert!(sanitized.contains("model = \"gpt-5.4\""));
        assert!(sanitized.contains("[features]"));
        assert!(sanitized.contains("[notice]"));
        assert!(!sanitized.contains("notify ="));
        assert!(!sanitized.contains("[mcp_servers.slack]"));
        assert!(!sanitized.contains("command = \"docker\""));
    }

    #[test]
    fn test_sanitize_claude_settings_strips_hooks() {
        let settings = r#"{
  "permissions": {
    "defaultMode": "plan"
  },
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "$HOME/.local/bin/ai-waiting-beep"
          }
        ]
      }
    ]
  }
}"#;

        let sanitized = sanitize_claude_settings(settings);
        assert!(sanitized.contains("\"defaultMode\": \"plan\""));
        assert!(!sanitized.contains("\"hooks\""));
        assert!(!sanitized.contains("ai-waiting-beep"));
    }

    #[test]
    fn test_sanitize_claude_settings_returns_original_when_invalid_json() {
        let invalid = "{ invalid";
        assert_eq!(sanitize_claude_settings(invalid), invalid);
    }

    #[test]
    fn test_prepare_runtime_home_in_stages_sanitized_codex_config()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions"))?;
        fs::write(
            host_codex_home.join("config.toml"),
            r#"
model = "gpt-5.4"
notify = ["bash", "-lc", "beep"]

[mcp_servers.slack]
command = "docker"
"#,
        )?;
        fs::write(host_codex_home.join("auth.json"), r#"{"token":"abc"}"#)?;

        let prepared = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )?;

        assert!(prepared.home_source.join(".cache").is_dir());
        assert!(prepared.home_source.join(".config").is_dir());
        assert!(host_codex_home.join("sessions").is_dir());

        let managed_config = fs::read_to_string(prepared.codex_home_source.join("config.toml"))?;
        assert!(managed_config.contains("model = \"gpt-5.4\""));
        assert!(!managed_config.contains("notify ="));
        assert!(!managed_config.contains("[mcp_servers.slack]"));

        let managed_auth = fs::read_to_string(prepared.codex_home_source.join("auth.json"))?;
        assert_eq!(managed_auth, r#"{"token":"abc"}"#);
        Ok(())
    }

    #[test]
    fn test_prepare_runtime_home_in_stages_claude_config() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = TempDir::new()?;
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_claude_dir = host_home.join(".claude");
        fs::create_dir_all(host_claude_dir.join("commands"))?;
        fs::create_dir_all(host_home.join(".codex").join("sessions"))?;
        fs::write(
            host_home.join(".claude.json"),
            r#"{"oauthAccount":{"email":"q@example.com"}}"#,
        )?;
        fs::write(
            host_claude_dir.join(".credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"abc"}}"#,
        )?;
        fs::write(
            host_claude_dir.join("settings.json"),
            r#"{
  "permissions": {
    "defaultMode": "plan"
  },
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "$HOME/.local/bin/ai-waiting-beep"
          }
        ]
      }
    ]
  }
}"#,
        )?;
        fs::write(
            host_claude_dir.join("commands").join("review.md"),
            "# review\n",
        )?;
        fs::write(
            host_home.join(".codex").join("config.toml"),
            "model = \"gpt-5.4\"\n",
        )?;

        let prepared = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_home.join(".codex"),
        )?;

        let managed_settings =
            fs::read_to_string(prepared.home_source.join(".claude").join("settings.json"))?;
        assert!(managed_settings.contains("\"defaultMode\": \"plan\""));
        assert!(!managed_settings.contains("\"hooks\""));
        assert!(!managed_settings.contains("ai-waiting-beep"));

        let managed_credentials = fs::read_to_string(
            prepared
                .home_source
                .join(".claude")
                .join(".credentials.json"),
        )?;
        assert!(managed_credentials.contains("accessToken"));

        let managed_claude_json = fs::read_to_string(prepared.home_source.join(".claude.json"))?;
        assert!(managed_claude_json.contains("oauthAccount"));

        let managed_command = fs::read_to_string(
            prepared
                .home_source
                .join(".claude")
                .join("commands/review.md"),
        )?;
        assert_eq!(managed_command, "# review\n");
        Ok(())
    }

    #[test]
    fn test_sync_optional_dir_removes_target_when_source_missing()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let source = temp.path().join("missing-source");
        let target = temp.path().join("target");
        fs::create_dir_all(&target)?;
        fs::write(target.join("stale.txt"), "stale")?;

        sync_optional_dir(&source, &target)?;

        assert!(!target.exists());
        Ok(())
    }

    #[test]
    fn test_prepare_runtime_home_in_creates_managed_cargo_home()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions"))?;

        let prepared = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )?;

        assert!(prepared.home_source.join(".cargo").is_dir());
        Ok(())
    }

    #[test]
    fn test_prepare_runtime_home_in_refreshes_staged_auth_and_settings()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        let host_claude_dir = host_home.join(".claude");
        fs::create_dir_all(host_codex_home.join("sessions"))?;
        fs::create_dir_all(&host_claude_dir)?;

        fs::write(host_codex_home.join("auth.json"), r#"{"token":"old"}"#)?;
        fs::write(
            host_claude_dir.join("settings.json"),
            r#"{"permissions":{"defaultMode":"plan"},"hooks":{"Stop":[]}}"#,
        )?;

        let prepared = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )?;

        assert_eq!(
            fs::read_to_string(prepared.codex_home_source.join("auth.json"))?,
            r#"{"token":"old"}"#
        );

        fs::write(host_codex_home.join("auth.json"), r#"{"token":"new"}"#)?;
        fs::write(
            host_claude_dir.join("settings.json"),
            r#"{"permissions":{"defaultMode":"acceptEdits"}}"#,
        )?;

        let refreshed = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )?;

        assert_eq!(
            fs::read_to_string(refreshed.codex_home_source.join("auth.json"))?,
            r#"{"token":"new"}"#
        );
        assert!(
            fs::read_to_string(refreshed.home_source.join(".claude").join("settings.json"))?
                .contains("\"defaultMode\": \"acceptEdits\"")
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_mounts_runtime_env_and_symlink_targets()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let (log_contents, prepared, repo_root, host_home) = docker_mount_log_for_test(&temp)?;
        let expected_target_mount = repo_root.join("target").canonicalize()?;
        let expected_plan_mount = repo_root.join("PLAN.md").canonicalize()?;
        assert!(log_contents.contains(&format!(
            "-v {}:{}",
            repo_root.join(".git").display(),
            repo_root.join(".git").display()
        )));
        assert!(log_contents.contains(&format!(
            "-v {}:{}",
            prepared.home_source.display(),
            host_home.display()
        )));
        assert!(log_contents.contains(&format!(
            "-v {}:{}",
            prepared.codex_home_source.display(),
            prepared.codex_home_target.display()
        )));
        assert!(log_contents.contains(&format!(
            "CARGO_HOME={}",
            host_home.join(".cargo").display()
        )));
        assert!(log_contents.contains(&format!(
            "-v {}:{}",
            expected_target_mount.display(),
            expected_target_mount.display()
        )));
        assert!(log_contents.contains(&format!(
            "-v {}:{}",
            expected_plan_mount.display(),
            expected_plan_mount.display()
        )));
        assert!(!log_contents.contains(&format!(
            "-v {}:{}",
            host_home.join(".claude").display(),
            host_home.join(".claude").display()
        )));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_rejects_running_stale_layout() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = TempDir::new()?;
        let host_home = temp.path().join("host-home");
        let data_local_dir = temp.path().join("xdg-data");
        fs::create_dir_all(host_home.join(".codex").join("sessions"))?;
        fs::create_dir_all(host_home.join(".claude"))?;
        fs::write(
            host_home.join(".codex").join("config.toml"),
            "model = \"gpt-5.4\"\n",
        )?;
        fs::write(
            host_home.join(".claude").join("settings.json"),
            "{\"permissions\":{\"defaultMode\":\"plan\"}}",
        )?;
        let inspect_count = temp.path().join("inspect-count");
        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  count=0\n  if [ -f '{}' ]; then\n    count=$(cat '{}')\n  fi\n  count=$((count + 1))\n  printf '%s' \"$count\" > '{}'\n  if [ \"$count\" -eq 1 ]; then\n    printf '%s\\n' 'true'\n  else\n    printf '%s\\n' 'stale-layout'\n  fi\n  exit 0\nfi\nexit 0\n",
                default_worker_dockerfile_hash(),
                inspect_count.display(),
                inspect_count.display(),
                inspect_count.display()
            ),
        )?;

        with_docker_program_override_for_tests(script, || {
            let result = ensure_container_with_paths(
                &docker_agent(),
                &Settings::default(),
                Some(&host_home),
                Some(&data_local_dir),
                None,
            );
            assert!(result.is_err());
            let err = result
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(err.contains("older Tenex worker layout"));
        });
        Ok(())
    }

    #[cfg(unix)]
    fn docker_mount_log_for_test(
        temp: &TempDir,
    ) -> Result<(String, PreparedRuntimeHome, PathBuf, PathBuf), Box<dyn std::error::Error>> {
        let log = temp.path().join("docker.log");
        let script = write_fake_docker_script(
            temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  echo 'No such object' >&2\n  exit 1\nfi\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
                log.display()
            ),
        )?;

        let repo_root = temp.path().join("repo");
        let worktree = temp.path().join("worktree");
        let host_home = temp.path().join("host-home");
        let data_local_dir = temp.path().join("xdg-data");
        fs::create_dir_all(repo_root.join("target"))?;
        fs::create_dir_all(
            repo_root
                .join(".git")
                .join("worktrees")
                .join("agent-docker"),
        )?;
        fs::create_dir_all(&worktree)?;
        fs::create_dir_all(host_home.join(".codex").join("sessions"))?;
        fs::create_dir_all(host_home.join(".claude"))?;
        fs::write(repo_root.join("PLAN.md"), "# plan\n")?;
        std::os::unix::fs::symlink(repo_root.join("target"), worktree.join("target"))?;
        std::os::unix::fs::symlink(repo_root.join("PLAN.md"), worktree.join("PLAN.md"))?;
        fs::write(
            host_home.join(".codex").join("config.toml"),
            "model = \"gpt-5.4\"\n",
        )?;
        fs::write(
            host_home.join(".claude").join("settings.json"),
            "{\"hooks\":{\"Stop\":[]}}",
        )?;

        let mut agent = docker_agent();
        agent.repo_root = Some(repo_root.clone());
        agent.worktree_path = worktree.clone();

        let prepared = prepare_runtime_home_in(
            &agent,
            &host_home,
            &data_local_dir,
            &host_home.join(".codex"),
        )?;

        with_docker_program_override_for_tests(script, || {
            let settings = Settings::default();
            let image = worker_image_tag(&settings);
            let mut cmd = docker_command();
            cmd.args([
                "run",
                "-d",
                "--init",
                "--name",
                &container_name(&agent),
                "--hostname",
                &container_name(&agent),
            ]);
            if let Some(user) = docker_user_arg() {
                cmd.args(["--user", &user]);
            }
            cmd.arg("--label").arg(format!(
                "{WORKER_CONTAINER_LAYOUT_HASH_LABEL}={}",
                current_container_layout_hash()
            ));
            cmd.arg("-e").arg(format!("HOME={}", host_home.display()));
            cmd.arg("-e").arg(format!(
                "XDG_CACHE_HOME={}",
                host_home.join(".cache").display()
            ));
            cmd.arg("-e")
                .arg(format!("CARGO_HOME={}", host_home.join(".cargo").display()));
            cmd.arg("-e")
                .arg(format!("CODEX_HOME={}", host_home.join(".codex").display()));
            add_bind_mount(&mut cmd, &prepared.home_source, &host_home, false);
            add_bind_mount(
                &mut cmd,
                &prepared.codex_home_source,
                &prepared.codex_home_target,
                false,
            );
            add_bind_mount(
                &mut cmd,
                &prepared.codex_home_target.join("sessions"),
                &prepared.codex_home_target.join("sessions"),
                false,
            );
            configure_repo_metadata_mounts(&mut cmd, &agent);
            cmd.args(["-w", &display_path(&worktree)]);
            add_bind_mount(&mut cmd, &worktree, &worktree, false);
            cmd.arg(image);
            cmd.arg("sleep");
            cmd.arg("infinity");
            run_command(&mut cmd, "Failed to create Docker container")
        })?;

        Ok((fs::read_to_string(&log)?, prepared, repo_root, host_home))
    }
}
