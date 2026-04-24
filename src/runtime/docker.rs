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
const WORKER_CONTAINER_LAYOUT_VERSION: &str = "5";
const NSS_WRAPPER_LIB_PATH: &str = "/usr/local/lib/libnss_wrapper.so";
const RUNTIME_PASSWD_FILE_NAME: &str = ".tenex-passwd";
const RUNTIME_GROUP_FILE_NAME: &str = ".tenex-group";
const DEFAULT_RUNTIME_USER_NAME: &str = "tenex";
const DEFAULT_RUNTIME_GROUP_NAME: &str = "tenex";
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
            } else if let Some(home) = home {
                refresh_runtime_home_for_reuse_with_data_local_dir(agent, home, data_local_dir)?;
                if is_running {
                    return Ok(());
                }
                run_command(
                    docker_command().args(["start", &name]),
                    "Failed to start Docker container",
                )?;
                return Ok(());
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
    let home = paths::home_dir();
    exec_prefix_with_forwarded_env_and_home(agent, forwarded_env, home.as_deref())
}

fn exec_prefix_with_forwarded_env_and_home(
    agent: &Agent,
    forwarded_env: &[(&str, String)],
    home: Option<&Path>,
) -> Vec<String> {
    let worktree_target = container_target_path(&agent.worktree_path);
    let mut argv = vec![
        docker_program().to_string_lossy().into_owned(),
        "exec".to_string(),
        "-it".to_string(),
        "-w".to_string(),
        display_path(&worktree_target),
    ];

    if let Some(home) = home {
        let home_target = container_target_path(home);
        let codex_home = codex_home_dir(home);
        let codex_home_target = container_target_path(&codex_home);
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
        && bytes[2] == b'/'
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
    configure_runtime_identity_env(cmd, &home_target);

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

fn configure_runtime_identity_env(cmd: &mut Command, home_target: &Path) {
    let Some(identity) = current_runtime_user_info() else {
        return;
    };

    let passwd_target = home_target.join(RUNTIME_PASSWD_FILE_NAME);
    let group_target = home_target.join(RUNTIME_GROUP_FILE_NAME);

    cmd.arg("-e")
        .arg(format!("LD_PRELOAD={NSS_WRAPPER_LIB_PATH}"));
    cmd.arg("-e")
        .arg(format!("NSS_WRAPPER_PASSWD={}", passwd_target.display()));
    cmd.arg("-e")
        .arg(format!("NSS_WRAPPER_GROUP={}", group_target.display()));

    cmd.arg("-e").arg(format!("USER={}", identity.user_name));
    cmd.arg("-e").arg(format!("LOGNAME={}", identity.user_name));
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

    let paths = entries
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    configure_top_level_symlink_mounts_for_paths(cmd, worktree, mounted_targets, paths);
}

fn configure_top_level_symlink_mounts_for_paths(
    cmd: &mut Command,
    worktree: &Path,
    mounted_targets: &mut HashSet<PathBuf>,
    paths: Vec<PathBuf>,
) {
    let worktree = worktree
        .canonicalize()
        .unwrap_or_else(|_| worktree.to_path_buf());
    for path in paths {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_symlink() {
            continue;
        }

        let Ok(resolved) = path.canonicalize() else {
            continue;
        };
        if resolved.starts_with(&worktree) {
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

#[cfg(test)]
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

fn refresh_runtime_home_for_reuse_with_data_local_dir(
    agent: &Agent,
    home: &Path,
    data_local_dir: Option<&Path>,
) -> Result<()> {
    let data_local_dir = data_local_dir
        .map(Path::to_path_buf)
        .or_else(paths::data_local_dir)
        .or_else(paths::home_dir)
        .unwrap_or_else(std::env::temp_dir);
    let codex_home_target = codex_home_dir(home);
    refresh_runtime_home_for_reuse_in(agent, home, &data_local_dir, &codex_home_target)
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

    ensure_runtime_home_directories(&home_source, codex_home_target)?;
    write_runtime_identity_files(&home_source, &container_target_path(home))?;
    sync_ssh_home(&home_source, home)?;
    sync_claude_home(&home_source, home)?;
    sync_codex_home(&codex_home_source, codex_home_target)?;

    Ok(PreparedRuntimeHome {
        home_source,
        codex_home_source,
        codex_home_target: codex_home_target.to_path_buf(),
    })
}

fn refresh_runtime_home_for_reuse_in(
    agent: &Agent,
    home: &Path,
    data_local_dir: &Path,
    codex_home_target: &Path,
) -> Result<()> {
    let runtime_root = runtime_root_dir(agent, data_local_dir);
    let home_source = runtime_root.join("home");
    let codex_home_source = runtime_root.join("codex-home");

    ensure_runtime_home_directories(&home_source, codex_home_target)?;
    write_runtime_identity_files(&home_source, &container_target_path(home))?;
    sync_claude_home(&home_source, home)?;
    sync_codex_home(&codex_home_source, codex_home_target)?;
    Ok(())
}

fn ensure_runtime_home_directories(home_source: &Path, codex_home_target: &Path) -> Result<()> {
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
    Ok(())
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

fn sync_ssh_home(target_home: &Path, host_home: &Path) -> Result<()> {
    let host_ssh = host_home.join(".ssh");
    let target_ssh = target_home.join(".ssh");
    sync_optional_path_following_symlinks(&host_ssh, &target_ssh)?;

    let host_xdg_ssh = host_home.join(".config").join("ssh");
    let target_xdg_ssh = target_home.join(".config").join("ssh");
    sync_optional_path_following_symlinks(&host_xdg_ssh, &target_xdg_ssh)?;
    Ok(())
}

fn sync_optional_path_following_symlinks(source: &Path, target: &Path) -> Result<()> {
    remove_path_if_exists(target)?;
    if source.exists() {
        copy_path_recursive_following_symlinks(source, target)?;
    }

    Ok(())
}

fn copy_path_recursive_following_symlinks(source: &Path, target: &Path) -> Result<()> {
    let mut active_sources = HashSet::new();
    copy_path_recursive_following_symlinks_inner(source, target, &mut active_sources)
}

fn copy_path_recursive_following_symlinks_inner(
    source: &Path,
    target: &Path,
    active_sources: &mut HashSet<PathBuf>,
) -> Result<()> {
    let metadata = std::fs::metadata(source)
        .with_context(|| format!("Failed to read {}", source.display()))?;
    let canonical_source = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    if !active_sources.insert(canonical_source.clone()) {
        return Ok(());
    }

    let result = if metadata.is_dir() {
        std::fs::create_dir_all(target)
            .with_context(|| format!("Failed to create {}", target.display()))?;

        for entry in std::fs::read_dir(source)
            .with_context(|| format!("Failed to read {}", source.display()))?
        {
            #[cfg(test)]
            let entry = run_copy_path_recursive_following_symlinks_dir_entry_hook(entry);
            let entry = entry.context(format!("Failed to read {}", source.display()))?;
            let entry_path = entry.path();
            #[cfg(test)]
            run_copy_path_recursive_following_symlinks_before_metadata_hook(&entry_path);
            let entry_type = std::fs::symlink_metadata(&entry_path)
                .with_context(|| format!("Failed to read file type for {}", entry_path.display()))?
                .file_type();
            let target_path = target.join(entry.file_name());
            if entry_type.is_symlink() {
                let Ok(resolved) = entry_path.canonicalize() else {
                    continue;
                };
                copy_path_recursive_following_symlinks_inner(
                    &resolved,
                    &target_path,
                    active_sources,
                )?;
            } else if entry_type.is_dir() {
                copy_path_recursive_following_symlinks_inner(
                    &entry_path,
                    &target_path,
                    active_sources,
                )?;
            } else if entry_type.is_file() {
                copy_file_with_permissions(&entry_path, &target_path)?;
            }
        }

        set_staged_permissions(target, metadata.permissions())
    } else if metadata.is_file() {
        copy_file_with_permissions(source, target)
    } else {
        Ok(())
    };

    active_sources.remove(&canonical_source);
    result
}

fn copy_file_with_permissions(source: &Path, target: &Path) -> Result<()> {
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
    #[cfg(test)]
    run_copy_file_with_permissions_after_copy_hook(source);
    let permissions = std::fs::metadata(source)
        .with_context(|| format!("Failed to read {}", source.display()))?
        .permissions();
    set_staged_permissions(target, permissions)?;
    Ok(())
}

#[cfg(test)]
type RecursiveCopyHook = Option<Box<dyn Fn(&Path)>>;

#[cfg(test)]
type DirEntryResultHook = Option<
    Box<dyn FnMut(std::io::Result<std::fs::DirEntry>) -> std::io::Result<std::fs::DirEntry>>,
>;

#[cfg(test)]
std::thread_local! {
    static COPY_PATH_RECURSIVE_BEFORE_SYMLINK_METADATA_HOOK: std::cell::RefCell<RecursiveCopyHook> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
std::thread_local! {
    static COPY_PATH_RECURSIVE_DIR_ENTRY_HOOK: std::cell::RefCell<DirEntryResultHook> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
fn with_copy_path_recursive_before_symlink_metadata_hook<T>(
    hook: impl Fn(&Path) + 'static,
    f: impl FnOnce() -> T,
) -> T {
    COPY_PATH_RECURSIVE_BEFORE_SYMLINK_METADATA_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
        let result = f();
        *slot.borrow_mut() = None;
        result
    })
}

#[cfg(test)]
fn with_copy_path_recursive_following_symlinks_dir_entry_hook<T>(
    hook: impl FnMut(std::io::Result<std::fs::DirEntry>) -> std::io::Result<std::fs::DirEntry> + 'static,
    f: impl FnOnce() -> T,
) -> T {
    COPY_PATH_RECURSIVE_DIR_ENTRY_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
        let result = f();
        *slot.borrow_mut() = None;
        result
    })
}

#[cfg(test)]
fn run_copy_path_recursive_following_symlinks_dir_entry_hook(
    entry: std::io::Result<std::fs::DirEntry>,
) -> std::io::Result<std::fs::DirEntry> {
    COPY_PATH_RECURSIVE_DIR_ENTRY_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().as_mut() {
            hook(entry)
        } else {
            entry
        }
    })
}

#[cfg(test)]
fn run_copy_path_recursive_following_symlinks_before_metadata_hook(path: &Path) {
    COPY_PATH_RECURSIVE_BEFORE_SYMLINK_METADATA_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow().as_ref() {
            hook(path);
        }
    });
}

#[cfg(test)]
std::thread_local! {
    static COPY_DIR_RECURSIVE_BEFORE_SYMLINK_METADATA_HOOK: std::cell::RefCell<RecursiveCopyHook> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
std::thread_local! {
    static COPY_DIR_RECURSIVE_DIR_ENTRY_HOOK: std::cell::RefCell<DirEntryResultHook> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
fn with_copy_dir_recursive_before_symlink_metadata_hook<T>(
    hook: impl Fn(&Path) + 'static,
    f: impl FnOnce() -> T,
) -> T {
    COPY_DIR_RECURSIVE_BEFORE_SYMLINK_METADATA_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
        let result = f();
        *slot.borrow_mut() = None;
        result
    })
}

#[cfg(test)]
fn with_copy_dir_recursive_dir_entry_hook<T>(
    hook: impl FnMut(std::io::Result<std::fs::DirEntry>) -> std::io::Result<std::fs::DirEntry> + 'static,
    f: impl FnOnce() -> T,
) -> T {
    COPY_DIR_RECURSIVE_DIR_ENTRY_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
        let result = f();
        *slot.borrow_mut() = None;
        result
    })
}

#[cfg(test)]
fn run_copy_dir_recursive_dir_entry_hook(
    entry: std::io::Result<std::fs::DirEntry>,
) -> std::io::Result<std::fs::DirEntry> {
    COPY_DIR_RECURSIVE_DIR_ENTRY_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().as_mut() {
            hook(entry)
        } else {
            entry
        }
    })
}

#[cfg(test)]
fn run_copy_dir_recursive_before_symlink_metadata_hook(path: &Path) {
    COPY_DIR_RECURSIVE_BEFORE_SYMLINK_METADATA_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow().as_ref() {
            hook(path);
        }
    });
}

#[cfg(test)]
std::thread_local! {
    static COPY_FILE_WITH_PERMISSIONS_AFTER_COPY_HOOK: std::cell::RefCell<RecursiveCopyHook> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
fn with_copy_file_with_permissions_after_copy_hook<T>(
    hook: impl Fn(&Path) + 'static,
    f: impl FnOnce() -> T,
) -> T {
    COPY_FILE_WITH_PERMISSIONS_AFTER_COPY_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
        let result = f();
        *slot.borrow_mut() = None;
        result
    })
}

#[cfg(test)]
fn run_copy_file_with_permissions_after_copy_hook(source: &Path) {
    COPY_FILE_WITH_PERMISSIONS_AFTER_COPY_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow().as_ref() {
            hook(source);
        }
    });
}

fn set_staged_permissions(path: &Path, permissions: std::fs::Permissions) -> Result<()> {
    std::fs::set_permissions(path, owner_writable_permissions(permissions))
        .with_context(|| format!("Failed to set permissions on {}", path.display()))
}

#[cfg(unix)]
fn owner_writable_permissions(mut permissions: std::fs::Permissions) -> std::fs::Permissions {
    use std::os::unix::fs::PermissionsExt;

    permissions.set_mode(permissions.mode() | 0o200);
    permissions
}

#[cfg(not(unix))]
fn owner_writable_permissions(permissions: std::fs::Permissions) -> std::fs::Permissions {
    permissions
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("Failed to remove {}", path.display()))?;
    } else if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("Failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target)
        .with_context(|| format!("Failed to create {}", target.display()))?;

    for entry in
        std::fs::read_dir(source).with_context(|| format!("Failed to read {}", source.display()))?
    {
        #[cfg(test)]
        let entry = run_copy_dir_recursive_dir_entry_hook(entry);
        let entry = entry.context(format!("Failed to read {}", source.display()))?;
        let entry_path = entry.path();
        #[cfg(test)]
        run_copy_dir_recursive_before_symlink_metadata_hook(&entry_path);
        let entry_type = std::fs::symlink_metadata(&entry_path)
            .with_context(|| format!("Failed to read file type for {}", entry_path.display()))?
            .file_type();
        let target_path = target.join(entry.file_name());
        if entry_type.is_dir() {
            copy_dir_recursive(&entry_path, &target_path)?;
        } else if entry_type.is_file() {
            std::fs::copy(&entry_path, &target_path).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    entry_path.display(),
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

    let host_claude_json = host_home.join(".claude.json");
    let target_claude_json = target_home.join(".claude.json");
    sync_optional_file(&host_claude_json, &target_claude_json)?;

    let host_credentials = host_claude_dir.join(".credentials.json");
    let target_credentials = target_claude_dir.join(".credentials.json");
    sync_optional_file(&host_credentials, &target_credentials)?;

    let host_mcp_needs_auth_cache = host_claude_dir.join("mcp-needs-auth-cache.json");
    let target_mcp_needs_auth_cache = target_claude_dir.join("mcp-needs-auth-cache.json");
    sync_optional_file(&host_mcp_needs_auth_cache, &target_mcp_needs_auth_cache)?;

    let host_settings = host_claude_dir.join("settings.json");
    let target_settings = target_claude_dir.join("settings.json");
    sync_claude_settings_file(&host_settings, &target_settings)?;

    let host_settings_local = host_claude_dir.join("settings.local.json");
    let target_settings_local = target_claude_dir.join("settings.local.json");
    sync_claude_settings_file(&host_settings_local, &target_settings_local)?;

    for dir_name in ["agents", "commands", "output-styles", "skills"] {
        let source = host_claude_dir.join(dir_name);
        let target = target_claude_dir.join(dir_name);
        sync_optional_dir(&source, &target)?;
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

fn write_runtime_identity_files(home_source: &Path, home_target: &Path) -> Result<()> {
    let passwd_path = home_source.join(RUNTIME_PASSWD_FILE_NAME);
    let group_path = home_source.join(RUNTIME_GROUP_FILE_NAME);

    let Some(identity) = current_runtime_user_info() else {
        return Ok(());
    };

    let shell = "/bin/bash";
    let home_target_display = home_target.display().to_string();
    let mut passwd = if identity.uid == "0" {
        String::new()
    } else {
        String::from("root:x:0:0:root:/root:/bin/bash\n")
    };
    passwd.push_str(&identity.user_name);
    passwd.push_str(":x:");
    passwd.push_str(&identity.uid);
    passwd.push(':');
    passwd.push_str(&identity.gid);
    passwd.push_str(":Tenex runtime user:");
    passwd.push_str(&home_target_display);
    passwd.push(':');
    passwd.push_str(shell);
    passwd.push('\n');

    let mut group = if identity.gid == "0" {
        String::new()
    } else {
        String::from("root:x:0:\n")
    };
    group.push_str(&identity.group_name);
    group.push_str(":x:");
    group.push_str(&identity.gid);
    group.push_str(":\n");

    std::fs::write(&passwd_path, passwd)
        .with_context(|| format!("Failed to write {}", passwd_path.display()))?;
    std::fs::write(&group_path, group)
        .with_context(|| format!("Failed to write {}", group_path.display()))?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeUserIdentity {
    user_name: String,
    group_name: String,
    uid: String,
    gid: String,
}

fn current_runtime_user_info() -> Option<RuntimeUserIdentity> {
    #[cfg(unix)]
    {
        let uid = read_trimmed_stdout("id", ["-u"])?;
        let gid = read_trimmed_stdout("id", ["-g"])?;
        let user_name = sanitize_runtime_account_name(
            read_trimmed_stdout("id", ["-un"]).or_else(|| std::env::var("USER").ok()),
            DEFAULT_RUNTIME_USER_NAME,
        );
        let group_name = sanitize_runtime_account_name(
            read_trimmed_stdout("id", ["-gn"]).or_else(|| std::env::var("USER").ok()),
            DEFAULT_RUNTIME_GROUP_NAME,
        );

        Some(RuntimeUserIdentity {
            user_name,
            group_name,
            uid,
            gid,
        })
    }

    #[cfg(not(unix))]
    {
        None
    }
}

fn sanitize_runtime_account_name(value: Option<String>, fallback: &str) -> String {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .filter(|value| !value.contains(':'))
        .filter(|value| !value.contains('\n'))
        .filter(|value| !value.contains('\r'))
        .unwrap_or_else(|| fallback.to_string())
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

    let fallback = contents.to_string();
    serde_json::to_string_pretty(&value).unwrap_or(fallback)
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
    if program == "terminal" {
        return Ok(());
    }

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
        "layout:{WORKER_CONTAINER_LAYOUT_VERSION};image:{};cargo-home:.cargo;mounts:repo-git,repo-worktrees,external-symlink-targets,managed-ssh-home;nss-wrapper",
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

fn write_dockerfile_to_docker_build_stdin(
    stdin: Option<&mut dyn Write>,
    dockerfile: &str,
) -> Result<()> {
    let stdin = stdin.context("docker build stdin missing")?;
    stdin
        .write_all(dockerfile.as_bytes())
        .context("Failed to write built-in Dockerfile to docker build")?;
    Ok(())
}

fn wait_with_output_for_docker_build(
    wait: impl FnOnce() -> std::io::Result<std::process::Output>,
    program: &str,
    args: &str,
) -> Result<std::process::Output> {
    wait()
        .map_err(|err| anyhow::anyhow!("Failed to wait for Docker build: {program} {args}: {err}"))
}

#[cfg(test)]
std::thread_local! {
    static DOCKER_BUILD_WAIT_OVERRIDE: std::cell::RefCell<Option<std::io::Result<std::process::Output>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn with_docker_build_wait_override<T>(
    override_result: std::io::Result<std::process::Output>,
    f: impl FnOnce() -> T,
) -> T {
    DOCKER_BUILD_WAIT_OVERRIDE.with(|slot| {
        *slot.borrow_mut() = Some(override_result);
        let result = f();
        *slot.borrow_mut() = None;
        result
    })
}

#[cfg(test)]
fn take_docker_build_wait_override() -> Option<std::io::Result<std::process::Output>> {
    DOCKER_BUILD_WAIT_OVERRIDE.with(|slot| slot.borrow_mut().take())
}

fn build_default_image(image: &str) -> Result<()> {
    let dockerfile = default_worker_dockerfile();
    let context_dir = default_worker_build_context_dir()?;
    build_default_image_with_command(
        image,
        &dockerfile,
        &context_dir,
        Stdio::piped(),
        docker_command(),
    )
}

fn build_default_image_with_command(
    image: &str,
    dockerfile: &str,
    context_dir: &Path,
    stdin: Stdio,
    mut cmd: Command,
) -> Result<()> {
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
        &display_path(context_dir),
    ])
    .stdin(stdin)
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

    write_dockerfile_to_docker_build_stdin(
        child.stdin.as_mut().map(|stdin| stdin as &mut dyn Write),
        dockerfile,
    )?;

    let output = wait_with_output_for_docker_build(
        || {
            #[cfg(test)]
            if let Some(override_result) = take_docker_build_wait_override() {
                drop(child.stdin.take());
                let _ = child.wait();
                return override_result;
            }
            child.wait_with_output()
        },
        &program,
        &args,
    )?;

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
    #[cfg(test)]
    {
        let override_value = docker_user_override_store()
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let DockerUserOverride::Value(value) = override_value {
            return value;
        }
    }

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
#[derive(Clone)]
enum DockerUserOverride {
    Unset,
    Value(Option<String>),
}
#[cfg(test)]
static DOCKER_USER_OVERRIDE: OnceLock<RwLock<DockerUserOverride>> = OnceLock::new();

#[cfg(test)]
fn docker_program_override_store() -> &'static RwLock<Option<PathBuf>> {
    DOCKER_PROGRAM_OVERRIDE.get_or_init(|| RwLock::new(None))
}

#[cfg(test)]
fn docker_user_override_store() -> &'static RwLock<DockerUserOverride> {
    DOCKER_USER_OVERRIDE.get_or_init(|| RwLock::new(DockerUserOverride::Unset))
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
struct DockerUserOverrideGuard;

#[cfg(test)]
impl Drop for DockerUserOverrideGuard {
    fn drop(&mut self) {
        *docker_user_override_store()
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = DockerUserOverride::Unset;
    }
}

#[cfg(test)]
fn set_docker_user_override_for_tests(value: Option<String>) -> DockerUserOverrideGuard {
    *docker_user_override_store()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = DockerUserOverride::Value(value);
    DockerUserOverrideGuard
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
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::os::unix::net::UnixListener;
    use std::process::Command;
    use tempfile::TempDir;

    #[cfg(unix)]
    const ENSURE_CONTAINER_CHILD_FLAG: &str = "TENEX_DOCKER_ENSURE_CONTAINER_CHILD";
    #[cfg(unix)]
    const ENSURE_CONTAINER_WORKTREE_VAR: &str = "TENEX_DOCKER_ENSURE_CONTAINER_WORKTREE";
    #[cfg(unix)]
    const RUNTIME_IDENTITY_CHILD_FLAG: &str = "TENEX_DOCKER_RUNTIME_IDENTITY_CHILD";
    #[cfg(unix)]
    const RUNTIME_IDENTITY_HOME_SOURCE_VAR: &str = "TENEX_DOCKER_RUNTIME_IDENTITY_HOME_SOURCE";
    #[cfg(unix)]
    const RUNTIME_IDENTITY_ENV_CHILD_FLAG: &str = "TENEX_DOCKER_RUNTIME_IDENTITY_ENV_CHILD";
    #[cfg(unix)]
    const MOUNT_LOG_IDENTITY_CHILD_FLAG: &str = "TENEX_DOCKER_MOUNT_LOG_IDENTITY_CHILD";
    #[cfg(unix)]
    const BUILD_CONTEXT_FAILURE_CHILD_FLAG: &str = "TENEX_DOCKER_BUILD_CONTEXT_FAILURE_CHILD";
    #[cfg(unix)]
    const CANONICALIZE_FAILURE_CHILD_FLAG: &str = "TENEX_DOCKER_CANONICALIZE_FAILURE_CHILD";
    #[cfg(unix)]
    const RUNTIME_USER_FALLBACK_CHILD_FLAG: &str = "TENEX_DOCKER_RUNTIME_USER_FALLBACK_CHILD";
    #[cfg(unix)]
    const RUNTIME_IDENTITY_GID_FAILURE_CHILD_FLAG: &str =
        "TENEX_DOCKER_RUNTIME_IDENTITY_GID_FAILURE_CHILD";
    #[cfg(unix)]
    const DOCKER_USER_GID_FAILURE_CHILD_FLAG: &str = "TENEX_DOCKER_USER_GID_FAILURE_CHILD";

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
    fn test_container_name_replaces_invalid_characters() {
        let mut agent = docker_agent();
        agent.runtime_scope = "root:bad scope".to_string();
        assert_eq!(container_name(&agent), "tenex-runtime-root-bad-scope");
    }

    #[test]
    fn test_windows_container_target_from_str_maps_drive_paths() {
        let target = windows_container_target_from_str(r"C:\tenex\worktrees\repo");
        assert_eq!(target, PathBuf::from("/tenex-host/c/tenex/worktrees/repo"));
    }

    #[test]
    fn test_windows_container_target_from_str_falls_back_for_unc_like_paths() {
        let target = windows_container_target_from_str("//server/share/tenex");
        assert_eq!(target, PathBuf::from("/tenex-host/misc/server/share/tenex"));
    }

    #[test]
    fn test_windows_container_target_from_str_falls_back_for_empty_paths() {
        let target = windows_container_target_from_str("");
        assert_eq!(target, PathBuf::from("/tenex-host/misc"));
    }

    #[test]
    fn test_windows_container_target_from_str_falls_back_for_drive_paths_without_root_separator() {
        let target = windows_container_target_from_str("C:tenex");
        assert_eq!(target, PathBuf::from("/tenex-host/misc/C:tenex"));
    }

    #[test]
    fn test_windows_container_target_from_str_falls_back_for_non_alphabetic_drive_prefixes() {
        let target = windows_container_target_from_str("1:/tenex");
        assert_eq!(target, PathBuf::from("/tenex-host/misc/1/tenex"));
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
    fn test_exec_prefix_with_forwarded_env_sets_home_when_available() {
        let home = PathBuf::from("/tmp/tenex-test-home");
        let argv =
            exec_prefix_with_forwarded_env_and_home(&docker_agent(), &[], Some(home.as_path()));
        let home_target = container_target_path(&home);
        let codex_home_target = container_target_path(&codex_home_dir(&home));

        assert!(
            argv.iter()
                .any(|arg| arg == &format!("HOME={}", home_target.display()))
        );
        assert!(argv.iter().any(|arg| {
            arg == &format!("XDG_CACHE_HOME={}", home_target.join(".cache").display())
        }));
        assert!(
            argv.iter()
                .any(|arg| arg == &format!("CARGO_HOME={}", home_target.join(".cargo").display()))
        );
        assert!(
            argv.iter()
                .any(|arg| arg == &format!("CODEX_HOME={}", codex_home_target.display()))
        );
    }

    #[test]
    fn test_exec_prefix_with_forwarded_env_omits_home_when_home_is_missing() {
        let argv = exec_prefix_with_forwarded_env_and_home(&docker_agent(), &[], None);
        assert!(!argv.iter().any(|arg| arg.starts_with("HOME=")));
        assert!(!argv.iter().any(|arg| arg.starts_with("XDG_CACHE_HOME=")));
        assert!(!argv.iter().any(|arg| arg.starts_with("CARGO_HOME=")));
        assert!(!argv.iter().any(|arg| arg.starts_with("CODEX_HOME=")));
    }

    #[test]
    fn test_collect_forwarded_exec_env_filters_empty_values() {
        let by_key = std::collections::HashMap::from([
            ("TERM", "xterm-256color".to_string()),
            ("COLORTERM", String::new()),
            ("SSH_AUTH_SOCK", "/tmp/ssh-agent.sock".to_string()),
        ]);
        let values = collect_forwarded_exec_env(|key| by_key.get(key).cloned());

        assert_eq!(
            values,
            vec![
                ("TERM", "xterm-256color".to_string()),
                ("SSH_AUTH_SOCK", "/tmp/ssh-agent.sock".to_string()),
            ]
        );
    }

    #[test]
    fn test_collect_forwarded_exec_env_skips_missing_values() {
        let by_key = std::collections::HashMap::from([("TERM", "xterm-256color".to_string())]);
        let values = collect_forwarded_exec_env(|key| by_key.get(key).cloned());

        assert_eq!(values, vec![("TERM", "xterm-256color".to_string())]);
    }

    #[test]
    fn test_configure_ssh_auth_sock_mount_from_uses_container_target() {
        let temp = TempDir::new().unwrap();
        let ssh_auth_sock = temp.path().join("ssh-agent.sock");
        fs::write(&ssh_auth_sock, []).unwrap();
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
    }

    #[test]
    fn test_configure_ssh_auth_sock_mount_from_is_noop_when_socket_missing() {
        let temp = TempDir::new().unwrap();
        let ssh_auth_sock = temp.path().join("missing.sock");
        let mut cmd = Command::new("docker");

        configure_ssh_auth_sock_mount_from(&mut cmd, Some(&ssh_auth_sock));

        assert!(cmd.get_args().next().is_none());
    }

    #[test]
    fn test_add_optional_bind_mount_once_skips_duplicate_targets() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source.txt");
        fs::write(&source, "payload").unwrap();

        let target = temp.path().join("target.txt");
        let mut cmd = Command::new("docker");
        let mut mounted_targets = HashSet::new();

        add_optional_bind_mount_once(&mut cmd, &mut mounted_targets, &source, &target, false);
        add_optional_bind_mount_once(&mut cmd, &mut mounted_targets, &source, &target, false);
        add_optional_bind_mount_once(
            &mut cmd,
            &mut mounted_targets,
            &temp.path().join("missing.txt"),
            &temp.path().join("missing-target"),
            false,
        );

        let args = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mount_args = args.iter().filter(|arg| *arg == "-v").count();
        assert_eq!(mount_args, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_resolved_symlink_target_resolves_relative_target_outside_worktree() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        let external = temp.path().join("external");
        let path = worktree.join("PLAN.md");
        let target = external.join("PLAN.md");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(&external).unwrap();
        fs::write(&target, "# plan\n").unwrap();

        let resolved = resolved_symlink_target(&path, &worktree, Path::new("../external/PLAN.md"));

        assert_eq!(resolved, target.canonicalize().unwrap());
    }

    #[cfg(unix)]
    fn write_fake_docker_script(temp: &TempDir, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("docker");
        fs::write(&script, body).unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
        script
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
    fn test_check_available_reports_spawn_errors_clearly() {
        let temp = TempDir::new().unwrap();
        let script = temp.path().join("docker");
        fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&script, perms).unwrap();

        with_docker_program_override_for_tests(script, || {
            let result = check_available();
            assert!(result.is_err());
            let err = result
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(err.contains("Failed to run `docker version`"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_check_available_reports_docker_version_failure_clearly() {
        let temp = TempDir::new().unwrap();
        let script_body = "#!/bin/sh\nif [ \"$1\" = \"version\" ]; then\n  echo 'Cannot connect to the Docker daemon' >&2\n  exit 1\nfi\nexit 0\n";
        let script = write_fake_docker_script(&temp, script_body);

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
    }

    #[cfg(unix)]
    #[test]
    fn test_check_available_prefers_stdout_when_stderr_is_empty() {
        let temp = TempDir::new().unwrap();
        let script_body = "#!/bin/sh\nif [ \"$1\" = \"version\" ]; then\n  printf '%s\\n' 'stdout failure'\n  exit 1\nfi\nexit 0\n";
        let script = write_fake_docker_script(&temp, script_body);

        with_docker_program_override_for_tests(script, || {
            let result = check_available();
            assert!(result.is_err());
            let err = result
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(err.contains("Docker is unavailable: `docker version` failed"));
            assert!(err.contains("stdout failure"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_check_available_reports_generic_detail_when_output_is_empty() {
        let temp = TempDir::new().unwrap();
        let script_body = "#!/bin/sh\nif [ \"$1\" = \"version\" ]; then\n  exit 1\nfi\nexit 0\n";
        let script = write_fake_docker_script(&temp, script_body);

        with_docker_program_override_for_tests(script, || {
            let result = check_available();
            assert!(result.is_err());
            let err = result
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(err.contains("Docker is unavailable: `docker version` failed"));
            assert!(err.contains("the Docker daemon may be unavailable"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_image_ready_builds_shipped_image_when_missing() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("docker.log");
        let dockerfile = temp.path().join("Dockerfile");
        let script_body = format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  echo 'No such image' >&2\n  exit 1\nfi\nif [ \"$1\" = \"build\" ]; then\n  cat > \"{}\"\n  exit 0\nfi\nexit 0\n",
            log.display(),
            dockerfile.display(),
        );
        let script = write_fake_docker_script(&temp, &script_body);

        with_docker_program_override_for_tests(script, || {
            let result = ensure_image_ready(&Settings::default(), "codex");
            assert!(result.is_ok());
        });

        let log_contents = std::fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains("image inspect --format"));
        assert!(log_contents.contains(WORKER_IMAGE_TEMPLATE_HASH_LABEL));
        assert!(log_contents.contains("build --tag tenex-worker:latest --label"));
        assert!(log_contents.contains(WORKER_IMAGE_TEMPLATE_HASH_LABEL));
        assert!(log_contents.contains(&default_worker_dockerfile_hash()));
        assert!(log_contents.contains("--file -"));

        let dockerfile_contents = std::fs::read_to_string(&dockerfile).unwrap();
        assert!(dockerfile_contents.contains("@openai/codex"));
        assert!(dockerfile_contents.contains("@anthropic-ai/claude-code"));
        assert!(dockerfile_contents.contains("rustup component add clippy llvm-tools rustfmt"));
        assert!(
            dockerfile_contents.contains("cargo install cargo-llvm-cov --locked --version 0.6.22")
        );
        assert!(dockerfile_contents.contains("libnss-wrapper"));
        assert!(dockerfile_contents.contains("openssh-client"));
        assert!(dockerfile_contents.contains("/usr/local/lib/libnss_wrapper.so"));
        assert!(dockerfile_contents.contains("/etc/profile.d/tenex-rust-path.sh"));
        assert!(dockerfile_contents.contains("/usr/local/cargo/bin/*"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_image_ready_rebuilds_stale_shipped_image() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("docker.log");
        let script_body = format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' 'stale-hash'\n  exit 0\nfi\nif [ \"$1\" = \"build\" ]; then\n  cat >/dev/null\n  exit 0\nfi\nexit 0\n",
            log.display(),
        );
        let script = write_fake_docker_script(&temp, &script_body);

        with_docker_program_override_for_tests(script, || {
            let result = ensure_image_ready(&Settings::default(), "codex");
            assert!(result.is_ok());
        });

        let log_contents = std::fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains("image inspect --format"));
        assert!(log_contents.contains("build --tag tenex-worker:latest --label"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_image_ready_noops_when_image_is_up_to_date() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("docker.log");
        let script_body = format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"build\" ]; then\n  printf '%s\\n' 'unexpected build' >&2\n  exit 1\nfi\nexit 0\n",
            log.display(),
            default_worker_dockerfile_hash(),
        );
        let script = write_fake_docker_script(&temp, &script_body);

        with_docker_program_override_for_tests(script, || {
            let result = ensure_image_ready(&Settings::default(), "codex");
            assert!(result.is_ok());
        });

        let log_contents = std::fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains("image inspect --format"));
        assert!(!log_contents.contains("build --tag"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_image_ready_propagates_unexpected_image_inspect_errors() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  echo 'boom' >&2\n  exit 1\nfi\nif [ \"$1\" = \"build\" ]; then\n  echo 'unexpected build' >&2\n  exit 1\nfi\nexit 0\n",
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_image_ready(&Settings::default(), "codex").unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to inspect Docker image"));
        assert!(message.contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn test_image_build_required_returns_false_when_image_is_up_to_date() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 0\n",
                default_worker_dockerfile_hash(),
            ),
        );

        with_docker_program_override_for_tests(script, || {
            assert!(!image_build_required(&Settings::default(), "codex").unwrap());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_image_build_required_returns_true_when_image_missing() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  echo 'No such image' >&2\n  exit 1\nfi\nexit 0\n",
        );

        with_docker_program_override_for_tests(script, || {
            assert!(image_build_required(&Settings::default(), "codex").unwrap());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_image_build_required_propagates_unexpected_image_inspect_errors() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  echo 'boom' >&2\n  exit 1\nfi\nexit 0\n",
        );

        let err = with_docker_program_override_for_tests(script, || {
            image_build_required(&Settings::default(), "codex").unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to inspect Docker image"));
        assert!(message.contains("boom"));
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
    fn test_sanitize_codex_config_strips_root_mcp_servers_table() {
        let config = r#"
[mcp_servers]
command = "docker"

[notice]
hide_rate_limit_model_nudge = true
"#;

        let sanitized = sanitize_codex_config(config);
        assert!(sanitized.contains("[notice]"));
        assert!(!sanitized.contains("[mcp_servers]"));
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
    fn test_sanitize_claude_settings_keeps_non_object_values() {
        let settings = "[1, 2, 3]";
        let sanitized = sanitize_claude_settings(settings);
        let parsed: serde_json::Value = serde_json::from_str(&sanitized).expect("valid json");
        assert_eq!(parsed, serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn test_sanitize_claude_settings_returns_original_when_invalid_json() {
        let invalid = "{ invalid";
        assert_eq!(sanitize_claude_settings(invalid), invalid);
    }

    #[cfg(unix)]
    #[test]
    fn test_container_matches_current_layout_uses_runtime_label() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 0\n",
                current_container_layout_hash(),
            ),
        );

        with_docker_program_override_for_tests(script, || {
            assert_eq!(
                container_layout_hash("tenex-runtime-test").unwrap(),
                Some(current_container_layout_hash())
            );
            assert!(container_matches_current_layout("tenex-runtime-test").unwrap());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_image_template_hash_returns_none_when_label_missing() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '<no value>'\n  exit 0\nfi\nexit 0\n",
        );

        with_docker_program_override_for_tests(script, || {
            assert_eq!(image_template_hash("tenex-worker:latest").unwrap(), None);
            assert!(!image_matches_default_template("tenex-worker:latest").unwrap());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_image_template_hash_returns_none_when_output_is_empty() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' ''\n  exit 0\nfi\nexit 0\n",
        );

        with_docker_program_override_for_tests(script, || {
            assert_eq!(image_template_hash("tenex-worker:latest").unwrap(), None);
            assert!(!image_matches_default_template("tenex-worker:latest").unwrap());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_image_template_hash_returns_none_when_image_missing_reports_no_such_object() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' 'No such object' >&2\n  exit 1\nfi\nexit 0\n",
        );

        with_docker_program_override_for_tests(script, || {
            assert_eq!(image_template_hash("tenex-worker:latest").unwrap(), None);
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_image_template_hash_propagates_unexpected_errors() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  echo 'boom' >&2\n  exit 1\nfi\nexit 0\n",
        );

        let err = with_docker_program_override_for_tests(script, || {
            image_template_hash("tenex-worker:latest").unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to inspect Docker image"));
        assert!(message.contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn test_container_layout_hash_returns_none_when_label_missing() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"inspect\" ]; then\n  printf '%s\\n' '<no value>'\n  exit 0\nfi\nexit 0\n",
        );

        with_docker_program_override_for_tests(script, || {
            assert_eq!(container_layout_hash("tenex-runtime-test").unwrap(), None);
            assert!(!container_matches_current_layout("tenex-runtime-test").unwrap());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_container_layout_hash_returns_none_when_output_is_empty() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"inspect\" ]; then\n  printf '%s\\n' ''\n  exit 0\nfi\nexit 0\n",
        );

        with_docker_program_override_for_tests(script, || {
            assert_eq!(container_layout_hash("tenex-runtime-test").unwrap(), None);
            assert!(!container_matches_current_layout("tenex-runtime-test").unwrap());
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_container_layout_hash_returns_none_when_container_missing() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"inspect\" ]; then\n  echo 'No such container' >&2\n  exit 1\nfi\nexit 0\n",
        );

        with_docker_program_override_for_tests(script, || {
            assert_eq!(container_layout_hash("tenex-runtime-test").unwrap(), None);
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_container_layout_hash_returns_none_when_container_missing_reports_no_such_object() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"inspect\" ]; then\n  echo 'No such object' >&2\n  exit 1\nfi\nexit 0\n",
        );

        with_docker_program_override_for_tests(script, || {
            assert_eq!(container_layout_hash("tenex-runtime-test").unwrap(), None);
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_container_layout_hash_propagates_unexpected_errors() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"inspect\" ]; then\n  echo 'boom' >&2\n  exit 1\nfi\nexit 0\n",
        );

        let err = with_docker_program_override_for_tests(script, || {
            container_layout_hash("tenex-runtime-test").unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to inspect Docker container"));
        assert!(message.contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_image_ready_reports_build_failure() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  echo 'No such image' >&2\n  exit 1\nfi\nif [ \"$1\" = \"build\" ]; then\n  cat >/dev/null\n  echo 'build failed' >&2\n  exit 1\nfi\nexit 0\n",
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_image_ready(&Settings::default(), "codex").unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to build built-in Tenex worker image"));
        assert!(message.contains("build failed"));
    }

    #[test]
    fn test_prepare_runtime_home_in_stages_sanitized_codex_config() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();
        let config_contents = r#"
model = "gpt-5.4"
notify = ["bash", "-lc", "beep"]

[mcp_servers.slack]
command = "docker"
"#;
        fs::write(host_codex_home.join("config.toml"), config_contents).unwrap();
        fs::write(host_codex_home.join("auth.json"), r#"{"token":"abc"}"#).unwrap();

        let prepared = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )
        .unwrap();

        assert!(prepared.home_source.join(".cache").is_dir());
        assert!(prepared.home_source.join(".config").is_dir());
        assert!(host_codex_home.join("sessions").is_dir());

        let managed_config =
            fs::read_to_string(prepared.codex_home_source.join("config.toml")).unwrap();
        assert!(managed_config.contains("model = \"gpt-5.4\""));
        assert!(!managed_config.contains("notify ="));
        assert!(!managed_config.contains("[mcp_servers.slack]"));

        let managed_auth =
            fs::read_to_string(prepared.codex_home_source.join("auth.json")).unwrap();
        assert_eq!(managed_auth, r#"{"token":"abc"}"#);
    }

    #[test]
    fn test_prepare_runtime_home_in_stages_claude_config() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_claude_dir = host_home.join(".claude");
        fs::create_dir_all(host_claude_dir.join("commands")).unwrap();
        fs::create_dir_all(host_home.join(".codex").join("sessions")).unwrap();
        fs::write(
            host_home.join(".claude.json"),
            r#"{"oauthAccount":{"email":"q@example.com"}}"#,
        )
        .unwrap();
        fs::write(
            host_claude_dir.join(".credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"abc"}}"#,
        )
        .unwrap();
        let settings_contents = r#"{
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
        fs::write(host_claude_dir.join("settings.json"), settings_contents).unwrap();
        fs::write(
            host_claude_dir.join("commands").join("review.md"),
            "# review\n",
        )
        .unwrap();
        fs::write(
            host_home.join(".codex").join("config.toml"),
            "model = \"gpt-5.4\"\n",
        )
        .unwrap();

        let prepared = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_home.join(".codex"),
        )
        .unwrap();

        let managed_settings =
            fs::read_to_string(prepared.home_source.join(".claude").join("settings.json")).unwrap();
        assert!(managed_settings.contains("\"defaultMode\": \"plan\""));
        assert!(!managed_settings.contains("\"hooks\""));
        assert!(!managed_settings.contains("ai-waiting-beep"));

        let credentials_path = prepared
            .home_source
            .join(".claude")
            .join(".credentials.json");
        let managed_credentials = fs::read_to_string(credentials_path).unwrap();
        assert!(managed_credentials.contains("accessToken"));

        let managed_claude_json =
            fs::read_to_string(prepared.home_source.join(".claude.json")).unwrap();
        assert!(managed_claude_json.contains("oauthAccount"));

        let command_path = prepared
            .home_source
            .join(".claude")
            .join("commands/review.md");
        let managed_command = fs::read_to_string(command_path).unwrap();
        assert_eq!(managed_command, "# review\n");
    }

    #[test]
    fn test_sync_optional_dir_removes_target_when_source_missing() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("missing-source");
        let target = temp.path().join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("stale.txt"), "stale").unwrap();

        sync_optional_dir(&source, &target).unwrap();

        assert!(!target.exists());
    }

    #[test]
    fn test_sync_optional_dir_replaces_existing_target_when_source_present() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("fresh.txt"), "fresh").unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("stale.txt"), "stale").unwrap();

        sync_optional_dir(&source, &target).unwrap();

        assert!(!target.join("stale.txt").exists());
        assert_eq!(
            fs::read_to_string(target.join("fresh.txt")).unwrap(),
            "fresh"
        );
    }

    #[test]
    fn test_sync_codex_home_reports_target_creation_failure() {
        let temp = TempDir::new().unwrap();
        let host_codex_home = temp.path().join("host-codex-home");
        fs::create_dir_all(&host_codex_home).unwrap();

        let target = temp.path().join("managed-codex-home");
        fs::write(&target, "not-a-directory").unwrap();

        let err = sync_codex_home(&target, &host_codex_home).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to create managed Codex home directory"));
        assert!(message.contains(&target.display().to_string()));
    }

    #[test]
    fn test_sync_codex_home_removes_stale_config_when_source_missing() {
        let temp = TempDir::new().unwrap();
        let host_codex_home = temp.path().join("host-codex-home");
        fs::create_dir_all(&host_codex_home).unwrap();

        let target = temp.path().join("managed-codex-home");
        fs::create_dir_all(&target).unwrap();

        let config_target = target.join("config.toml");
        fs::write(&config_target, "stale").unwrap();
        assert!(config_target.exists());

        sync_codex_home(&target, &host_codex_home).unwrap();

        assert!(!config_target.exists());
    }

    #[test]
    fn test_sync_codex_home_reports_remove_stale_config_failure() {
        let temp = TempDir::new().unwrap();
        let host_codex_home = temp.path().join("host-codex-home");
        fs::create_dir_all(&host_codex_home).unwrap();

        let target = temp.path().join("managed-codex-home");
        fs::create_dir_all(&target).unwrap();

        let config_target = target.join("config.toml");
        fs::create_dir_all(&config_target).unwrap();

        let err = sync_codex_home(&target, &host_codex_home).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to remove"));
        assert!(message.contains("config.toml"));
    }

    #[test]
    fn test_sync_optional_file_copies_file_and_creates_parent() {
        let temp = TempDir::new().unwrap();
        let source_dir = temp.path().join("source");
        fs::create_dir_all(&source_dir).unwrap();

        let source = source_dir.join("auth.json");
        fs::write(&source, "payload").unwrap();

        let target = temp.path().join("target").join("nested").join("auth.json");
        sync_optional_file(&source, &target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "payload");
    }

    #[test]
    fn test_sync_optional_file_reports_copy_failure() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source.json");
        fs::write(&source, "payload").unwrap();

        let target = temp.path().join("target.json");
        fs::create_dir_all(&target).unwrap();

        let err = sync_optional_file(&source, &target).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
        assert!(message.contains(&source.display().to_string()));
        assert!(message.contains(&target.display().to_string()));
    }

    #[test]
    fn test_sync_optional_file_removes_target_when_source_missing() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("missing-source.json");
        let target = temp.path().join("target.json");
        fs::write(&target, "stale").unwrap();
        assert!(target.exists());

        sync_optional_file(&source, &target).unwrap();

        assert!(!target.exists());
    }

    #[test]
    fn test_sync_optional_file_reports_remove_failure() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("missing-source.json");
        let target = temp.path().join("target.json");
        fs::create_dir_all(&target).unwrap();

        let err = sync_optional_file(&source, &target).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to remove"));
        assert!(message.contains(&target.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_optional_path_following_symlinks_replaces_stale_target_file() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let external_key = temp.path().join("external-key");
        fs::create_dir_all(&source).unwrap();
        fs::write(&target, "stale-target").unwrap();
        fs::write(source.join("config"), "Host staged\n").unwrap();
        fs::write(&external_key, "private-key\n").unwrap();
        std::os::unix::fs::symlink(&external_key, source.join("id_test")).unwrap();

        sync_optional_path_following_symlinks(&source, &target).unwrap();

        assert_eq!(
            fs::read_to_string(target.join("config")).unwrap(),
            "Host staged\n"
        );
        assert_eq!(
            fs::read_to_string(target.join("id_test")).unwrap(),
            "private-key\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_optional_path_following_symlinks_skips_broken_symlinks() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        std::os::unix::fs::symlink(temp.path().join("missing"), source.join("broken")).unwrap();

        sync_optional_path_following_symlinks(&source, &target).unwrap();

        assert!(target.is_dir());
        assert!(!target.join("broken").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_optional_path_following_symlinks_removes_target_when_source_missing() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("missing-source");
        let target = temp.path().join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("stale.txt"), "stale").unwrap();

        sync_optional_path_following_symlinks(&source, &target).unwrap();

        assert!(!target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_file_with_permissions_reports_missing_source() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("missing-source");
        let target = temp.path().join("target").join("copied");

        let err = copy_file_with_permissions(&source, &target)
            .expect_err("expected missing source error");
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
        assert!(message.contains(&source.display().to_string()));
        assert!(message.contains(&target.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_reports_missing_source() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("missing-source");
        let target = temp.path().join("target");

        let err = copy_path_recursive_following_symlinks(&source, &target)
            .expect_err("expected missing source error");
        let message = err.to_string();
        assert!(message.contains("Failed to read"));
        assert!(message.contains(&source.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn test_set_staged_permissions_reports_missing_path() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("missing-path");

        let err = set_staged_permissions(&path, fs::Permissions::from_mode(0o644))
            .expect_err("expected missing path error");
        let message = err.to_string();
        assert!(message.contains("Failed to set permissions on"));
        assert!(message.contains(&path.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_runtime_home_directories_reports_create_failure() {
        let temp = TempDir::new().unwrap();
        let cache_home_source = temp.path().join("runtime-home-cache");
        let cache_codex_home_target = temp.path().join("codex-home-cache");
        fs::write(&cache_home_source, "not-a-directory").unwrap();

        let err = ensure_runtime_home_directories(&cache_home_source, &cache_codex_home_target)
            .expect_err("expected directory creation failure");
        let message = err.to_string();
        assert!(message.contains("Failed to create Docker runtime cache directory"));
        assert!(message.contains(&cache_home_source.join(".cache").display().to_string()));

        let cargo_home_source = temp.path().join("runtime-home-cargo");
        let cargo_codex_home_target = temp.path().join("codex-home-cargo");
        fs::create_dir_all(&cargo_home_source).unwrap();
        fs::write(cargo_home_source.join(".cargo"), "not-a-directory").unwrap();

        let err = ensure_runtime_home_directories(&cargo_home_source, &cargo_codex_home_target)
            .expect_err("expected Cargo directory creation failure");
        let message = err.to_string();
        assert!(message.contains("Failed to create Docker runtime Cargo directory"));
        assert!(message.contains(&cargo_home_source.join(".cargo").display().to_string()));

        let config_home_source = temp.path().join("runtime-home-config");
        let config_codex_home_target = temp.path().join("codex-home-config");
        fs::create_dir_all(&config_home_source).unwrap();
        fs::write(config_home_source.join(".config"), "not-a-directory").unwrap();

        let err = ensure_runtime_home_directories(&config_home_source, &config_codex_home_target)
            .expect_err("expected config directory creation failure");
        let message = err.to_string();
        assert!(message.contains("Failed to create Docker runtime config directory"));
        assert!(message.contains(&config_home_source.join(".config").display().to_string()));

        let local_share_home_source = temp.path().join("runtime-home-local-share");
        let local_share_codex_home_target = temp.path().join("codex-home-local-share");
        fs::create_dir_all(&local_share_home_source).unwrap();
        fs::write(local_share_home_source.join(".local"), "not-a-directory").unwrap();

        let err = ensure_runtime_home_directories(
            &local_share_home_source,
            &local_share_codex_home_target,
        )
        .expect_err("expected local share directory creation failure");
        let message = err.to_string();
        assert!(message.contains("Failed to create Docker runtime local share directory"));
        assert!(
            message.contains(
                &local_share_home_source
                    .join(".local")
                    .join("share")
                    .display()
                    .to_string()
            )
        );

        let sessions_home_source = temp.path().join("runtime-home-sessions");
        let sessions_codex_home_target = temp.path().join("codex-home-sessions");
        fs::create_dir_all(&sessions_home_source).unwrap();
        fs::write(&sessions_codex_home_target, "not-a-directory").unwrap();

        let err =
            ensure_runtime_home_directories(&sessions_home_source, &sessions_codex_home_target)
                .expect_err("expected sessions directory creation failure");
        let message = err.to_string();
        assert!(message.contains("Failed to create host Codex sessions directory"));
        assert!(
            message.contains(
                &sessions_codex_home_target
                    .join("sessions")
                    .display()
                    .to_string()
            )
        );
    }

    #[test]
    fn test_sanitize_runtime_account_name_rejects_invalid_values() {
        assert_eq!(
            sanitize_runtime_account_name(Some("  alice  ".to_string()), "fallback"),
            "alice"
        );
        assert_eq!(
            sanitize_runtime_account_name(Some(String::new()), "fallback"),
            "fallback"
        );
        assert_eq!(
            sanitize_runtime_account_name(Some("alice:staff".to_string()), "fallback"),
            "fallback"
        );
        assert_eq!(
            sanitize_runtime_account_name(Some("ali\nce".to_string()), "fallback"),
            "fallback"
        );
        assert_eq!(sanitize_runtime_account_name(None, "fallback"), "fallback");
    }

    #[cfg(unix)]
    #[test]
    fn test_read_trimmed_stdout_trims_and_rejects_blank_output() {
        assert_eq!(
            read_trimmed_stdout("sh", ["-c", "printf '  value  \\n'"]),
            Some("value".to_string())
        );
        assert_eq!(read_trimmed_stdout("sh", ["-c", "printf '   \\n'"]), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_read_trimmed_stdout_returns_none_when_command_fails() {
        assert_eq!(read_trimmed_stdout("sh", ["-c", "exit 1"]), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_read_trimmed_stdout_returns_none_when_output_is_invalid_utf8() {
        assert_eq!(read_trimmed_stdout("sh", ["-c", "printf '\\377'"]), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_configure_runtime_identity_env_sets_expected_variables() {
        let mut cmd = Command::new("docker");
        let home_target = PathBuf::from("/tmp/runtime-home");

        configure_runtime_identity_env(&mut cmd, &home_target);

        let identity =
            current_runtime_user_info().expect("expected runtime user info on unix test host");
        let args = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.contains(&"-e".to_string()));
        assert!(args.contains(&format!("LD_PRELOAD={NSS_WRAPPER_LIB_PATH}")));
        assert!(args.contains(&format!(
            "NSS_WRAPPER_PASSWD={}",
            home_target.join(RUNTIME_PASSWD_FILE_NAME).display()
        )));
        assert!(args.contains(&format!(
            "NSS_WRAPPER_GROUP={}",
            home_target.join(RUNTIME_GROUP_FILE_NAME).display()
        )));
        assert!(args.contains(&format!("USER={}", identity.user_name)));
        assert!(args.contains(&format!("LOGNAME={}", identity.user_name)));
    }

    #[cfg(unix)]
    #[test]
    fn test_configure_runtime_identity_env_is_noop_when_user_info_unavailable() {
        if std::env::var_os(RUNTIME_IDENTITY_ENV_CHILD_FLAG).is_some() {
            let mut cmd = Command::new("docker");
            configure_runtime_identity_env(&mut cmd, Path::new("/tmp/runtime-home"));
            assert!(cmd.get_args().next().is_none());
            return;
        }

        let current_exe = std::env::current_exe().unwrap();
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg("runtime::docker::tests::test_configure_runtime_identity_env_is_noop_when_user_info_unavailable")
            .arg("--nocapture")
            .env(RUNTIME_IDENTITY_ENV_CHILD_FLAG, "1")
            .env("PATH", "")
            .output()
            .unwrap();

        assert!(output.status.success());
    }

    #[cfg(unix)]
    #[test]
    fn test_current_runtime_user_info_falls_back_to_env_user_for_missing_name_queries() {
        if std::env::var_os(RUNTIME_USER_FALLBACK_CHILD_FLAG).is_some() {
            let identity = current_runtime_user_info().expect("expected runtime user info");
            assert_eq!(identity.uid, "1234");
            assert_eq!(identity.gid, "5678");
            assert_eq!(identity.user_name, "tenex-test-user");
            assert_eq!(identity.group_name, "tenex-test-user");
            return;
        }

        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let id_script = bin_dir.join("id");
        fs::write(
            &id_script,
            "#!/bin/sh\ncase \"$1\" in\n  -u) echo 1234 ;;\n  -g) echo 5678 ;;\n  -un) exit 1 ;;\n  -gn) exit 1 ;;\n  *) exit 1 ;;\nesac\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&id_script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&id_script, perms).unwrap();

        let current_exe = std::env::current_exe().unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        let prefixed_path = format!("{}:{path}", bin_dir.display());
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg("runtime::docker::tests::test_current_runtime_user_info_falls_back_to_env_user_for_missing_name_queries")
            .arg("--nocapture")
            .env(RUNTIME_USER_FALLBACK_CHILD_FLAG, "1")
            .env("PATH", prefixed_path)
            .env("USER", "tenex-test-user")
            .output()
            .unwrap();

        assert!(output.status.success());
    }

    #[test]
    fn test_prepare_runtime_home_in_creates_managed_cargo_home() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();

        let prepared = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )
        .unwrap();

        assert!(prepared.home_source.join(".cargo").is_dir());
    }

    #[test]
    fn test_sync_codex_and_claude_home_helpers_copy_expected_files() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let target_home = temp.path().join("target-home");
        let host_codex_home = host_home.join(".codex");
        let target_codex_home = target_home.join(".codex");
        let host_claude_dir = host_home.join(".claude");
        let target_claude_dir = target_home.join(".claude");

        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();
        fs::create_dir_all(host_claude_dir.join("commands")).unwrap();
        fs::create_dir_all(&target_codex_home).unwrap();
        fs::create_dir_all(&target_claude_dir).unwrap();

        fs::write(
            host_codex_home.join("config.toml"),
            "model = \"gpt-5.4\"\nnotify = [\"beep\"]\n",
        )
        .unwrap();
        fs::write(host_codex_home.join("auth.json"), r#"{"token":"abc"}"#).unwrap();
        fs::write(
            host_claude_dir.join("settings.json"),
            r#"{"permissions":{"defaultMode":"plan"},"hooks":{"Stop":[]}}"#,
        )
        .unwrap();
        fs::write(
            host_claude_dir.join("commands").join("review.md"),
            "# review\n",
        )
        .unwrap();
        fs::write(host_home.join(".claude.json"), r#"{"oauthAccount":{}}"#).unwrap();

        sync_codex_home(&target_codex_home, &host_codex_home).unwrap();
        sync_claude_home(&target_home, &host_home).unwrap();

        let managed_codex_config =
            fs::read_to_string(target_codex_home.join("config.toml")).unwrap();
        assert!(managed_codex_config.contains("model = \"gpt-5.4\""));
        assert!(!managed_codex_config.contains("notify ="));
        assert_eq!(
            fs::read_to_string(target_codex_home.join("auth.json")).unwrap(),
            r#"{"token":"abc"}"#
        );

        let managed_claude_settings =
            fs::read_to_string(target_claude_dir.join("settings.json")).unwrap();
        assert!(managed_claude_settings.contains("\"defaultMode\": \"plan\""));
        assert!(!managed_claude_settings.contains("\"hooks\""));
        assert_eq!(
            fs::read_to_string(target_claude_dir.join("commands").join("review.md")).unwrap(),
            "# review\n"
        );
        assert_eq!(
            fs::read_to_string(target_home.join(".claude.json")).unwrap(),
            r#"{"oauthAccount":{}}"#
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_codex_home_reports_config_read_errors() {
        let temp = TempDir::new().unwrap();
        let host_codex_home = temp.path().join("host-codex");
        let target_codex_home = temp.path().join("target-codex");
        fs::create_dir_all(&host_codex_home).unwrap();
        fs::create_dir_all(&target_codex_home).unwrap();

        let config_source = host_codex_home.join("config.toml");
        fs::write(&config_source, "model = \"gpt-5.4\"\n").unwrap();
        let mut perms = fs::metadata(&config_source).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&config_source, perms).unwrap();

        let err = sync_codex_home(&target_codex_home, &host_codex_home).unwrap_err();
        assert!(err.to_string().contains("Failed to read"));

        let mut reset = fs::metadata(&config_source).unwrap().permissions();
        reset.set_mode(0o644);
        fs::set_permissions(&config_source, reset).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_codex_home_reports_config_write_errors() {
        let temp = TempDir::new().unwrap();
        let host_codex_home = temp.path().join("host-codex");
        let target_codex_home = temp.path().join("target-codex");
        fs::create_dir_all(&host_codex_home).unwrap();
        fs::create_dir_all(&target_codex_home).unwrap();

        fs::write(host_codex_home.join("config.toml"), "model = \"gpt-5.4\"\n").unwrap();
        let mut target_perms = fs::metadata(&target_codex_home).unwrap().permissions();
        target_perms.set_mode(0o555);
        fs::set_permissions(&target_codex_home, target_perms).unwrap();

        let err = sync_codex_home(&target_codex_home, &host_codex_home).unwrap_err();
        assert!(err.to_string().contains("Failed to write"));

        let mut reset = fs::metadata(&target_codex_home).unwrap().permissions();
        reset.set_mode(0o755);
        fs::set_permissions(&target_codex_home, reset).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_codex_home_propagates_optional_file_copy_errors() {
        let temp = TempDir::new().unwrap();
        let host_codex_home = temp.path().join("host-codex");
        let target_codex_home = temp.path().join("target-codex");
        fs::create_dir_all(&host_codex_home).unwrap();
        fs::create_dir_all(&target_codex_home).unwrap();

        fs::write(host_codex_home.join("auth.json"), r#"{"token":"abc"}"#).unwrap();

        let mut perms = fs::metadata(&target_codex_home).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&target_codex_home, perms).unwrap();

        let err = sync_codex_home(&target_codex_home, &host_codex_home).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
        assert!(message.contains("auth.json"));

        let mut reset = fs::metadata(&target_codex_home).unwrap().permissions();
        reset.set_mode(0o755);
        fs::set_permissions(&target_codex_home, reset).unwrap();
    }

    #[test]
    fn test_sync_optional_dir_reports_remove_errors_when_target_is_file() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("file.txt"), "hello").unwrap();
        fs::write(&target, "block").unwrap();

        let err = sync_optional_dir(&source, &target).unwrap_err();
        assert!(err.to_string().contains("Failed to remove"));
    }

    #[test]
    fn test_sync_optional_dir_reports_remove_errors_when_source_missing() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("missing-source");
        let target = temp.path().join("target");
        fs::write(&target, "block").unwrap();

        let err = sync_optional_dir(&source, &target).unwrap_err();
        assert!(err.to_string().contains("Failed to remove"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_optional_dir_propagates_copy_dir_recursive_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(source.join("file.txt"), "hello").unwrap();

        let err = with_copy_dir_recursive_before_symlink_metadata_hook(
            |path| {
                let _ = fs::remove_file(path);
            },
            || sync_optional_dir(&source, &target).unwrap_err(),
        );

        assert!(err.to_string().contains("Failed to read file type for"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_home_reports_create_errors_when_target_home_is_file() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let target_home = temp.path().join("target-home");
        fs::create_dir_all(&host_home).unwrap();
        fs::write(&target_home, "not-a-dir").unwrap();

        let err = sync_claude_home(&target_home, &host_home).unwrap_err();
        assert!(err.to_string().contains("Failed to create"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_home_propagates_failures_from_claude_json_copy() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let target_home = temp.path().join("target-home");
        fs::create_dir_all(&host_home).unwrap();
        fs::create_dir_all(&target_home).unwrap();
        fs::write(host_home.join(".claude.json"), r#"{"oauthAccount":{}}"#).unwrap();
        fs::create_dir_all(target_home.join(".claude.json")).unwrap();

        let err = sync_claude_home(&target_home, &host_home).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
        assert!(message.contains(".claude.json"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_home_propagates_failures_from_credentials_copy() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let target_home = temp.path().join("target-home");
        let host_claude_dir = host_home.join(".claude");
        let target_claude_dir = target_home.join(".claude");
        fs::create_dir_all(&host_claude_dir).unwrap();
        fs::create_dir_all(&target_claude_dir).unwrap();
        fs::write(host_claude_dir.join(".credentials.json"), "{}").unwrap();
        fs::create_dir_all(target_claude_dir.join(".credentials.json")).unwrap();

        let err = sync_claude_home(&target_home, &host_home).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
        assert!(message.contains(".credentials.json"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_home_propagates_failures_from_mcp_needs_auth_cache_copy() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let target_home = temp.path().join("target-home");
        let host_claude_dir = host_home.join(".claude");
        let target_claude_dir = target_home.join(".claude");
        fs::create_dir_all(&host_claude_dir).unwrap();
        fs::create_dir_all(&target_claude_dir).unwrap();
        fs::write(host_claude_dir.join("mcp-needs-auth-cache.json"), "{}").unwrap();
        fs::create_dir_all(target_claude_dir.join("mcp-needs-auth-cache.json")).unwrap();

        let err = sync_claude_home(&target_home, &host_home).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
        assert!(message.contains("mcp-needs-auth-cache.json"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_home_propagates_failures_from_settings_copy() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let target_home = temp.path().join("target-home");
        let host_claude_dir = host_home.join(".claude");
        let target_claude_dir = target_home.join(".claude");
        fs::create_dir_all(&host_claude_dir).unwrap();
        fs::create_dir_all(&target_claude_dir).unwrap();
        fs::write(
            host_claude_dir.join("settings.json"),
            r#"{"permissions":{"defaultMode":"plan"}}"#,
        )
        .unwrap();
        fs::create_dir_all(target_claude_dir.join("settings.json")).unwrap();

        let err = sync_claude_home(&target_home, &host_home).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to write"));
        assert!(message.contains("settings.json"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_home_propagates_failures_from_settings_local_copy() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let target_home = temp.path().join("target-home");
        let host_claude_dir = host_home.join(".claude");
        let target_claude_dir = target_home.join(".claude");
        fs::create_dir_all(&host_claude_dir).unwrap();
        fs::create_dir_all(&target_claude_dir).unwrap();
        fs::write(
            host_claude_dir.join("settings.local.json"),
            r#"{"permissions":{"defaultMode":"plan"}}"#,
        )
        .unwrap();
        fs::create_dir_all(target_claude_dir.join("settings.local.json")).unwrap();

        let err = sync_claude_home(&target_home, &host_home).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to write"));
        assert!(message.contains("settings.local.json"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_home_propagates_failures_from_optional_dir_sync() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let target_home = temp.path().join("target-home");
        let host_claude_dir = host_home.join(".claude");
        let target_claude_dir = target_home.join(".claude");
        fs::create_dir_all(host_claude_dir.join("agents")).unwrap();
        fs::create_dir_all(&target_claude_dir).unwrap();
        fs::write(target_claude_dir.join("agents"), "blocker").unwrap();

        let err = sync_claude_home(&target_home, &host_home).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to remove"));
        assert!(message.contains("agents"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_settings_file_reports_read_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("settings.json");
        let target = temp.path().join("target.json");
        fs::write(&source, "{\"permissions\":{}}").unwrap();
        let mut perms = fs::metadata(&source).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&source, perms).unwrap();

        let err = sync_claude_settings_file(&source, &target).unwrap_err();
        assert!(err.to_string().contains("Failed to read"));

        let mut reset = fs::metadata(&source).unwrap().permissions();
        reset.set_mode(0o644);
        fs::set_permissions(&source, reset).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_settings_file_reports_write_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("settings.json");
        let target = temp.path().join("target.json");
        fs::write(&source, "{\"permissions\":{}}").unwrap();
        fs::create_dir_all(&target).unwrap();

        let err = sync_claude_settings_file(&source, &target).unwrap_err();
        assert!(err.to_string().contains("Failed to write"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_settings_file_reports_remove_errors_for_directory_target() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("missing-source");
        let target = temp.path().join("target.json");
        fs::create_dir_all(&target).unwrap();

        let err = sync_claude_settings_file(&source, &target).unwrap_err();
        assert!(err.to_string().contains("Failed to remove"));
    }

    #[test]
    fn test_run_command_and_build_context_helpers() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf helper-output"]);
        assert_eq!(
            run_command(&mut command, "Failed to run helper command").unwrap(),
            "helper-output"
        );

        let build_context = default_worker_build_context_dir().unwrap();
        assert!(build_context.is_dir());
        assert!(build_context.ends_with("tenex/docker-build-context"));
    }

    #[test]
    fn test_run_command_reports_output_errors() {
        let mut command = Command::new("/definitely/missing/tenex-run-command");
        let err = run_command(&mut command, "Failed to run missing command").unwrap_err();
        assert!(err.to_string().contains("Failed to run missing command"));
        assert!(err.to_string().contains("tenex-run-command"));
    }

    #[cfg(unix)]
    #[test]
    fn test_default_worker_build_context_dir_reports_create_errors() {
        if std::env::var_os(BUILD_CONTEXT_FAILURE_CHILD_FLAG).is_some() {
            let base = paths::data_local_dir().expect("expected data local dir in child");
            fs::create_dir_all(&base).unwrap();
            fs::write(base.join("tenex"), "blocker").unwrap();
            let err = default_worker_build_context_dir().unwrap_err();
            assert!(
                err.to_string()
                    .contains("Failed to create Docker build context")
            );
            return;
        }

        let temp = TempDir::new().unwrap();
        let xdg = temp.path().join("xdg-data");
        fs::create_dir_all(&xdg).unwrap();
        let current_exe = std::env::current_exe().unwrap();
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg("runtime::docker::tests::test_default_worker_build_context_dir_reports_create_errors")
            .arg("--nocapture")
            .env(BUILD_CONTEXT_FAILURE_CHILD_FLAG, "1")
            .env("XDG_DATA_HOME", &xdg)
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    #[cfg(unix)]
    #[test]
    fn test_build_default_image_reports_build_context_create_errors() {
        if std::env::var_os(BUILD_CONTEXT_FAILURE_CHILD_FLAG).is_some() {
            let base = paths::data_local_dir().expect("expected data local dir in child");
            fs::create_dir_all(&base).unwrap();
            fs::write(base.join("tenex"), "blocker").unwrap();
            let err = build_default_image("tenex-worker:latest").unwrap_err();
            assert!(
                err.to_string()
                    .contains("Failed to create Docker build context")
            );
            return;
        }

        let temp = TempDir::new().unwrap();
        let xdg = temp.path().join("xdg-data");
        fs::create_dir_all(&xdg).unwrap();
        let current_exe = std::env::current_exe().unwrap();
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg("runtime::docker::tests::test_build_default_image_reports_build_context_create_errors")
            .arg("--nocapture")
            .env(BUILD_CONTEXT_FAILURE_CHILD_FLAG, "1")
            .env("XDG_DATA_HOME", &xdg)
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_inner_handles_canonicalize_failure() {
        if std::env::var_os(CANONICALIZE_FAILURE_CHILD_FLAG).is_some() {
            let temp = TempDir::new().unwrap();
            let parent = temp.path().join("parent");
            let source = parent.join("source");
            let target = parent.join("target");
            let cwd = parent.join("cwd");
            fs::create_dir_all(&source).unwrap();
            fs::create_dir_all(&target).unwrap();
            fs::create_dir_all(&cwd).unwrap();
            fs::write(source.join("file.txt"), "hello").unwrap();

            std::env::set_current_dir(&cwd).unwrap();
            fs::remove_dir(&cwd).unwrap();

            let mut active_sources = HashSet::new();
            copy_path_recursive_following_symlinks_inner(
                Path::new("../source"),
                Path::new("../target"),
                &mut active_sources,
            )
            .unwrap();

            std::env::set_current_dir(&parent).unwrap();
            assert_eq!(
                fs::read_to_string(target.join("file.txt")).unwrap(),
                "hello"
            );
            return;
        }

        let current_exe = std::env::current_exe().unwrap();
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg(
                "runtime::docker::tests::test_copy_path_recursive_following_symlinks_inner_handles_canonicalize_failure",
            )
            .arg("--nocapture")
            .env(CANONICALIZE_FAILURE_CHILD_FLAG, "1")
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_inner_reports_target_create_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(&source).unwrap();

        let blocker = temp.path().join("blocker");
        fs::write(&blocker, "blocker").unwrap();
        let target = blocker.join("target");
        let mut active_sources = HashSet::new();
        let err =
            copy_path_recursive_following_symlinks_inner(&source, &target, &mut active_sources)
                .unwrap_err();
        assert!(err.to_string().contains("Failed to create"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_inner_reports_read_dir_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();

        let mut perms = fs::metadata(&source).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&source, perms).unwrap();

        let mut active_sources = HashSet::new();
        let err =
            copy_path_recursive_following_symlinks_inner(&source, &target, &mut active_sources)
                .unwrap_err();
        assert!(err.to_string().contains("Failed to read"));

        let mut reset = fs::metadata(&source).unwrap().permissions();
        reset.set_mode(0o755);
        fs::set_permissions(&source, reset).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_inner_reports_symlink_metadata_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();

        fs::write(source.join("file.txt"), "hello").unwrap();

        let err = with_copy_path_recursive_before_symlink_metadata_hook(
            move |path| {
                let _ = fs::remove_file(path);
            },
            || {
                let mut active_sources = HashSet::new();
                copy_path_recursive_following_symlinks_inner(&source, &target, &mut active_sources)
                    .unwrap_err()
            },
        );
        assert!(err.to_string().contains("Failed to read file type for"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_file_with_permissions_reports_parent_create_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source.txt");
        fs::write(&source, "hello").unwrap();
        let blocker = temp.path().join("blocker");
        fs::write(&blocker, "blocker").unwrap();
        let target = blocker.join("nested").join("target.txt");

        let err = copy_file_with_permissions(&source, &target).unwrap_err();
        assert!(err.to_string().contains("Failed to create"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_file_with_permissions_reports_metadata_errors_after_copy() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source.txt");
        let target = temp.path().join("target.txt");
        fs::write(&source, "hello").unwrap();

        let err = with_copy_file_with_permissions_after_copy_hook(
            |source_path| {
                let _ = fs::remove_file(source_path);
            },
            || copy_file_with_permissions(&source, &target).unwrap_err(),
        );

        assert!(err.to_string().contains("Failed to read"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_file_with_permissions_reports_permissions_set_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source.txt");
        let target = temp.path().join("target.txt");
        fs::write(&source, "hello").unwrap();

        let hook_target = target.clone();
        let err = with_copy_file_with_permissions_after_copy_hook(
            move |_| {
                let _ = fs::remove_file(&hook_target);
            },
            || copy_file_with_permissions(&source, &target).unwrap_err(),
        );

        assert!(err.to_string().contains("Failed to set permissions on"));
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_path_if_exists_reports_remove_dir_errors() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("unremovable");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("file.txt"), "hello").unwrap();

        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&path, perms).unwrap();

        let err = remove_path_if_exists(&path).unwrap_err();
        assert!(err.to_string().contains("Failed to remove"));

        let mut reset = fs::metadata(&path).unwrap().permissions();
        reset.set_mode(0o755);
        fs::set_permissions(&path, reset).unwrap();
        fs::remove_dir_all(&path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_path_if_exists_reports_remove_file_errors() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join("parent");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("file.txt");
        fs::write(&file, "hello").unwrap();

        let mut perms = fs::metadata(&dir).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&dir, perms).unwrap();

        let err = remove_path_if_exists(&file).unwrap_err();
        assert!(err.to_string().contains("Failed to remove"));

        let mut reset = fs::metadata(&dir).unwrap().permissions();
        reset.set_mode(0o755);
        fs::set_permissions(&dir, reset).unwrap();
        fs::remove_file(&file).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_dir_recursive_reports_target_create_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("file.txt"), "hello").unwrap();

        let blocker = temp.path().join("blocker");
        fs::write(&blocker, "blocker").unwrap();
        let target = blocker.join("nested");
        let err = copy_dir_recursive(&source, &target).unwrap_err();
        assert!(err.to_string().contains("Failed to create"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_dir_recursive_reports_read_dir_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(source.join("file.txt"), "hello").unwrap();

        let mut perms = fs::metadata(&source).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&source, perms).unwrap();

        let err = copy_dir_recursive(&source, &target).unwrap_err();
        assert!(err.to_string().contains("Failed to read"));

        let mut reset = fs::metadata(&source).unwrap().permissions();
        reset.set_mode(0o755);
        fs::set_permissions(&source, reset).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_dir_recursive_reports_symlink_metadata_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();

        fs::write(source.join("file.txt"), "hello").unwrap();

        let err = with_copy_dir_recursive_before_symlink_metadata_hook(
            |path| {
                let _ = fs::remove_file(path);
            },
            || copy_dir_recursive(&source, &target).unwrap_err(),
        );
        assert!(err.to_string().contains("Failed to read file type for"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_dir_recursive_reports_dir_entry_read_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(source.join("file.txt"), "hello").unwrap();

        let err = with_copy_dir_recursive_dir_entry_hook(
            |_| Err(std::io::Error::other("boom")),
            || copy_dir_recursive(&source, &target).unwrap_err(),
        );
        let message = err.to_string();
        let message_with_causes = format!("{err:#}");
        assert!(message.contains("Failed to read"));
        assert!(message_with_causes.contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_dir_recursive_reports_recursive_call_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let nested = source.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("file.txt"), "hello").unwrap();
        fs::create_dir_all(target.join("nested").join("file.txt")).unwrap();

        let err = copy_dir_recursive(&source, &target).unwrap_err();
        assert!(err.to_string().contains("Failed to copy"));
    }

    #[cfg(unix)]
    #[test]
    fn test_write_runtime_identity_files_reports_passwd_write_errors() {
        let temp = TempDir::new().unwrap();
        let home_source = temp.path().join("home-source");
        fs::create_dir_all(&home_source).unwrap();
        fs::create_dir_all(home_source.join(RUNTIME_PASSWD_FILE_NAME)).unwrap();

        let err =
            write_runtime_identity_files(&home_source, Path::new("/tmp/runtime-home")).unwrap_err();
        assert!(err.to_string().contains("Failed to write"));
    }

    #[cfg(unix)]
    #[test]
    fn test_write_runtime_identity_files_reports_group_write_errors() {
        let temp = TempDir::new().unwrap();
        let home_source = temp.path().join("home-source");
        fs::create_dir_all(&home_source).unwrap();
        fs::create_dir_all(home_source.join(RUNTIME_GROUP_FILE_NAME)).unwrap();

        let err =
            write_runtime_identity_files(&home_source, Path::new("/tmp/runtime-home")).unwrap_err();
        assert!(err.to_string().contains("Failed to write"));
    }

    #[test]
    fn test_write_dockerfile_to_docker_build_stdin_reports_missing_stdin() {
        let err = write_dockerfile_to_docker_build_stdin(None, "FROM scratch").unwrap_err();
        assert!(err.to_string().contains("docker build stdin missing"));
    }

    #[test]
    fn test_write_dockerfile_to_docker_build_stdin_reports_write_failures() {
        struct FailingWriter;

        impl std::io::Write for FailingWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("boom"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut writer = FailingWriter;
        writer.flush().unwrap();
        let err = write_dockerfile_to_docker_build_stdin(
            Some(&mut writer),
            "FROM scratch\nRUN echo hello\n",
        )
        .unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("Failed to write built-in Dockerfile to docker build"));
        assert!(message.contains("boom"));
    }

    #[test]
    fn test_wait_with_output_for_docker_build_reports_wait_failures() {
        let err = wait_with_output_for_docker_build(
            || Err(std::io::Error::other("boom")),
            "docker",
            "build --tag tenex-worker:latest",
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to wait for Docker build"));
        assert!(message.contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn test_build_default_image_with_command_reports_wait_failures() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(&temp, "#!/bin/sh\ncat >/dev/null\nexit 0\n");
        let err = with_docker_program_override_for_tests(script, || {
            with_docker_build_wait_override(Err(std::io::Error::other("boom")), || {
                build_default_image_with_command(
                    "tenex-worker:latest",
                    "FROM scratch\n",
                    temp.path(),
                    Stdio::piped(),
                    docker_command(),
                )
                .unwrap_err()
            })
        });
        let message = err.to_string();
        assert!(message.contains("Failed to wait for Docker build"));
        assert!(message.contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn test_build_default_image_reports_docker_build_stdin_missing() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(&temp, "#!/bin/sh\nexit 0\n");
        let err = with_docker_program_override_for_tests(script, || {
            build_default_image_with_command(
                "tenex-worker:latest",
                "FROM scratch\n",
                temp.path(),
                Stdio::null(),
                docker_command(),
            )
            .unwrap_err()
        });
        assert!(err.to_string().contains("docker build stdin missing"));
    }

    #[cfg(unix)]
    #[test]
    fn test_build_default_image_reports_spawn_failures() {
        let missing = PathBuf::from("/definitely/missing/tenex-docker-build");
        let err = with_docker_program_override_for_tests(missing, || {
            build_default_image("tenex-worker:latest").unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to spawn Docker build"));
        assert!(message.contains("tenex-worker:latest"));
    }

    #[cfg(unix)]
    #[test]
    fn test_current_runtime_user_info_returns_none_when_gid_lookup_fails() {
        if std::env::var_os(RUNTIME_IDENTITY_GID_FAILURE_CHILD_FLAG).is_some() {
            assert!(current_runtime_user_info().is_none());
            return;
        }

        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let id_script = bin_dir.join("id");
        fs::write(
            &id_script,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"-u\" ]; then\n  printf '%s\\n' 1000\n  exit 0\nfi\nif [ \"$1\" = \"-g\" ]; then\n  exit 1\nfi\nexit 1\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&id_script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&id_script, perms).unwrap();

        let current_exe = std::env::current_exe().unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        let prefixed_path = format!("{}:{path}", bin_dir.display());
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg("runtime::docker::tests::test_current_runtime_user_info_returns_none_when_gid_lookup_fails")
            .arg("--nocapture")
            .env(RUNTIME_IDENTITY_GID_FAILURE_CHILD_FLAG, "1")
            .env("PATH", prefixed_path)
            .output()
            .unwrap();

        assert!(output.status.success());
    }

    #[cfg(unix)]
    #[test]
    fn test_docker_user_arg_returns_none_when_gid_lookup_fails() {
        if std::env::var_os(DOCKER_USER_GID_FAILURE_CHILD_FLAG).is_some() {
            assert!(docker_user_arg().is_none());
            return;
        }

        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let id_script = bin_dir.join("id");
        fs::write(
            &id_script,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"-u\" ]; then\n  printf '%s\\n' 1000\n  exit 0\nfi\nif [ \"$1\" = \"-g\" ]; then\n  exit 1\nfi\nexit 1\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&id_script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&id_script, perms).unwrap();

        let current_exe = std::env::current_exe().unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        let prefixed_path = format!("{}:{path}", bin_dir.display());
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg("runtime::docker::tests::test_docker_user_arg_returns_none_when_gid_lookup_fails")
            .arg("--nocapture")
            .env(DOCKER_USER_GID_FAILURE_CHILD_FLAG, "1")
            .env("PATH", prefixed_path)
            .output()
            .unwrap();

        assert!(output.status.success());
    }

    #[cfg(unix)]
    #[test]
    fn test_write_runtime_identity_files_preserve_existing_files_when_lookup_fails() {
        if std::env::var_os(RUNTIME_IDENTITY_CHILD_FLAG).is_some() {
            let home_source = PathBuf::from(
                std::env::var_os(RUNTIME_IDENTITY_HOME_SOURCE_VAR)
                    .expect("missing runtime identity home source"),
            );
            let passwd_path = home_source.join(RUNTIME_PASSWD_FILE_NAME);
            let group_path = home_source.join(RUNTIME_GROUP_FILE_NAME);
            let existing_passwd = fs::read_to_string(&passwd_path).unwrap();
            let existing_group = fs::read_to_string(&group_path).unwrap();

            write_runtime_identity_files(&home_source, Path::new("/tmp/runtime-home")).unwrap();

            assert_eq!(fs::read_to_string(passwd_path).unwrap(), existing_passwd);
            assert_eq!(fs::read_to_string(group_path).unwrap(), existing_group);
            return;
        }

        let temp = TempDir::new().unwrap();
        let home_source = temp.path().join("runtime-home");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&home_source).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(
            home_source.join(RUNTIME_PASSWD_FILE_NAME),
            "existing-passwd\n",
        )
        .unwrap();
        fs::write(
            home_source.join(RUNTIME_GROUP_FILE_NAME),
            "existing-group\n",
        )
        .unwrap();

        let id_script = bin_dir.join("id");
        fs::write(&id_script, "#!/bin/sh\nexit 1\n").unwrap();
        let mut perms = fs::metadata(&id_script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&id_script, perms).unwrap();

        let current_exe = std::env::current_exe().unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        let prefixed_path = format!("{}:{path}", bin_dir.display());
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg(
                "runtime::docker::tests::test_write_runtime_identity_files_preserve_existing_files_when_lookup_fails",
            )
            .arg("--nocapture")
            .env(RUNTIME_IDENTITY_CHILD_FLAG, "1")
            .env(RUNTIME_IDENTITY_HOME_SOURCE_VAR, &home_source)
            .env("PATH", prefixed_path)
            .output()
            .unwrap();

        assert!(output.status.success());
    }

    #[test]
    fn test_prepare_runtime_home_in_writes_runtime_identity_files() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();

        let prepared = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )
        .unwrap();

        let passwd =
            fs::read_to_string(prepared.home_source.join(RUNTIME_PASSWD_FILE_NAME)).unwrap();
        let group = fs::read_to_string(prepared.home_source.join(RUNTIME_GROUP_FILE_NAME)).unwrap();
        assert!(passwd.contains("Tenex runtime user"));
        assert!(passwd.contains(&container_target_path(&host_home).display().to_string()));
        assert!(group.contains(':'));
    }

    #[test]
    fn test_prepare_runtime_home_in_propagates_runtime_identity_write_errors() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();

        let agent = docker_agent();
        let runtime_root = runtime_root_dir(&agent, &data_local_dir);
        let home_source = runtime_root.join("home");
        fs::create_dir_all(&home_source).unwrap();
        fs::create_dir_all(home_source.join(RUNTIME_PASSWD_FILE_NAME)).unwrap();

        let err = prepare_runtime_home_in(&agent, &host_home, &data_local_dir, &host_codex_home)
            .err()
            .expect("expected prepare_runtime_home_in to fail");
        assert!(err.to_string().contains("Failed to write"));
    }

    #[cfg(unix)]
    #[test]
    fn test_prepare_runtime_home_in_propagates_sync_ssh_home_errors() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();

        let host_ssh = host_home.join(".ssh");
        fs::create_dir_all(&host_ssh).unwrap();
        fs::write(host_ssh.join("config"), "Host test\n").unwrap();

        let agent = docker_agent();
        let err = with_copy_path_recursive_before_symlink_metadata_hook(
            |path| {
                let _ = fs::remove_file(path);
            },
            || {
                prepare_runtime_home_in(&agent, &host_home, &data_local_dir, &host_codex_home)
                    .err()
                    .expect("expected prepare_runtime_home_in to fail")
            },
        );

        assert!(err.to_string().contains("Failed to read file type for"));
    }

    #[cfg(unix)]
    #[test]
    fn test_prepare_runtime_home_in_propagates_sync_claude_home_errors() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();

        let agent = docker_agent();
        let runtime_root = runtime_root_dir(&agent, &data_local_dir);
        let home_source = runtime_root.join("home");
        fs::create_dir_all(&home_source).unwrap();
        fs::write(home_source.join(".claude"), "blocker").unwrap();

        let err = prepare_runtime_home_in(&agent, &host_home, &data_local_dir, &host_codex_home)
            .err()
            .expect("expected prepare_runtime_home_in to fail");
        let message = err.to_string();
        assert!(message.contains("Failed to create"));
        assert!(message.contains(".claude"));
    }

    #[test]
    fn test_prepare_runtime_home_in_propagates_sync_codex_home_errors() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();

        let agent = docker_agent();
        let runtime_root = runtime_root_dir(&agent, &data_local_dir);
        fs::create_dir_all(&runtime_root).unwrap();
        fs::write(runtime_root.join("codex-home"), "blocker").unwrap();

        let err = prepare_runtime_home_in(&agent, &host_home, &data_local_dir, &host_codex_home)
            .err()
            .expect("expected prepare_runtime_home_in to fail");
        assert!(
            err.to_string()
                .contains("Failed to create managed Codex home directory")
        );
    }

    #[test]
    fn test_refresh_runtime_home_for_reuse_in_propagates_runtime_identity_write_errors() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();

        let agent = docker_agent();
        let runtime_root = runtime_root_dir(&agent, &data_local_dir);
        let home_source = runtime_root.join("home");
        fs::create_dir_all(&home_source).unwrap();
        fs::create_dir_all(home_source.join(RUNTIME_PASSWD_FILE_NAME)).unwrap();

        let err = refresh_runtime_home_for_reuse_in(
            &agent,
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Failed to write"));
    }

    #[cfg(unix)]
    #[test]
    fn test_refresh_runtime_home_for_reuse_in_propagates_sync_claude_home_errors() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();

        let agent = docker_agent();
        let runtime_root = runtime_root_dir(&agent, &data_local_dir);
        let home_source = runtime_root.join("home");
        fs::create_dir_all(&home_source).unwrap();
        fs::write(home_source.join(".claude"), "blocker").unwrap();

        let err = refresh_runtime_home_for_reuse_in(
            &agent,
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )
        .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to create"));
        assert!(message.contains(".claude"));
    }

    #[test]
    fn test_refresh_runtime_home_for_reuse_in_propagates_sync_codex_home_errors() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();

        let agent = docker_agent();
        let runtime_root = runtime_root_dir(&agent, &data_local_dir);
        fs::create_dir_all(&runtime_root).unwrap();
        fs::write(runtime_root.join("codex-home"), "blocker").unwrap();

        let err = refresh_runtime_home_for_reuse_in(
            &agent,
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to create managed Codex home directory")
        );
    }

    #[test]
    fn test_prepare_runtime_home_in_refreshes_staged_auth_and_settings() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        let host_claude_dir = host_home.join(".claude");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();
        fs::create_dir_all(&host_claude_dir).unwrap();

        fs::write(host_codex_home.join("auth.json"), r#"{"token":"old"}"#).unwrap();
        fs::write(
            host_claude_dir.join("settings.json"),
            r#"{"permissions":{"defaultMode":"plan"},"hooks":{"Stop":[]}}"#,
        )
        .unwrap();

        let prepared = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(prepared.codex_home_source.join("auth.json")).unwrap(),
            r#"{"token":"old"}"#
        );

        fs::write(host_codex_home.join("auth.json"), r#"{"token":"new"}"#).unwrap();
        fs::write(
            host_claude_dir.join("settings.json"),
            r#"{"permissions":{"defaultMode":"acceptEdits"}}"#,
        )
        .unwrap();

        let refreshed = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(refreshed.codex_home_source.join("auth.json")).unwrap(),
            r#"{"token":"new"}"#
        );
        assert!(
            fs::read_to_string(refreshed.home_source.join(".claude").join("settings.json"))
                .unwrap()
                .contains("\"defaultMode\": \"acceptEdits\"")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_refreshes_staged_auth_without_resetting_ssh_state() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let data_local_dir = temp.path().join("xdg-data");
        let worktree = temp.path().join("worktree");
        let host_codex_home = host_home.join(".codex");
        let host_claude_dir = host_home.join(".claude");
        let log = temp.path().join("docker.log");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();
        fs::create_dir_all(&host_claude_dir).unwrap();
        fs::create_dir_all(host_home.join(".ssh")).unwrap();
        fs::create_dir_all(host_home.join(".config").join("ssh")).unwrap();
        fs::write(host_codex_home.join("config.toml"), "model = \"gpt-5.4\"\n").unwrap();
        fs::write(host_codex_home.join("auth.json"), r#"{"token":"old"}"#).unwrap();
        fs::write(
            host_claude_dir.join("settings.json"),
            r#"{"permissions":{"defaultMode":"plan"},"hooks":{"Stop":[]}}"#,
        )
        .unwrap();
        fs::write(host_home.join(".ssh").join("config"), "Host test\n").unwrap();
        fs::write(host_home.join(".ssh").join("known_hosts"), "host-key\n").unwrap();
        fs::write(
            host_home.join(".config").join("ssh").join("config"),
            "Host xdg-test\n",
        )
        .unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let prepared =
            prepare_runtime_home_in(&agent, &host_home, &data_local_dir, &host_codex_home).unwrap();
        fs::write(
            prepared.home_source.join(".ssh").join("known_hosts"),
            "updated-host-key\n",
        )
        .unwrap();
        fs::write(host_codex_home.join("auth.json"), r#"{"token":"new"}"#).unwrap();
        fs::write(
            host_claude_dir.join("settings.json"),
            r#"{"permissions":{"defaultMode":"acceptEdits"}}"#,
        )
        .unwrap();

        let script_body = format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  if printf '%s' \"$3\" | grep -q '.State.Running'; then\n    printf '%s\\n' 'false'\n  else\n    printf '%s\\n' '{}'\n  fi\n  exit 0\nfi\nif [ \"$1\" = \"start\" ] || [ \"$1\" = \"rm\" ]; then\n  exit 0\nfi\nexit 0\n",
            log.display(),
            default_worker_dockerfile_hash(),
            current_container_layout_hash(),
        );
        let script = write_fake_docker_script(&temp, &script_body);

        with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(
                &agent,
                &Settings::default(),
                Some(&host_home),
                Some(&data_local_dir),
                None,
            )
            .unwrap();
        });

        assert_eq!(
            fs::read_to_string(prepared.codex_home_source.join("auth.json")).unwrap(),
            r#"{"token":"new"}"#
        );
        assert!(
            fs::read_to_string(prepared.home_source.join(".claude").join("settings.json"))
                .unwrap()
                .contains("\"defaultMode\": \"acceptEdits\"")
        );
        assert_eq!(
            fs::read_to_string(prepared.home_source.join(".ssh").join("known_hosts")).unwrap(),
            "updated-host-key\n"
        );

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains(&format!("start {}", container_name(&agent))));
        assert!(!log_contents.contains("run -d --init"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_reuses_running_container_when_home_is_available() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let data_local_dir = temp.path().join("xdg-data");
        let worktree = temp.path().join("worktree");
        let log = temp.path().join("docker.log");
        fs::create_dir_all(&host_home).unwrap();
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;
        let name = container_name(&agent);

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  if printf '%s' \"$3\" | grep -q '.State.Running'; then\n    printf '%s\\n' 'true'\n  else\n    printf '%s\\n' '{}'\n  fi\n  exit 0\nfi\nexit 0\n",
                log.display(),
                default_worker_dockerfile_hash(),
                current_container_layout_hash(),
            ),
        );

        with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(
                &agent,
                &Settings::default(),
                Some(&host_home),
                Some(&data_local_dir),
                None,
            )
            .unwrap();
        });

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(!log_contents.contains(&format!("start {name}")));
        assert!(!log_contents.contains(&format!("rm -f {name}")));
        assert!(!log_contents.contains("run -d --init"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_reports_start_failure_when_home_is_available() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let data_local_dir = temp.path().join("xdg-data");
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&host_home).unwrap();
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  if printf '%s' \"$3\" | grep -q '.State.Running'; then\n    printf '%s\\n' 'false'\n  else\n    printf '%s\\n' '{}'\n  fi\n  exit 0\nfi\nif [ \"$1\" = \"start\" ]; then\n  echo 'start failed' >&2\n  exit 1\nfi\nexit 0\n",
                default_worker_dockerfile_hash(),
                current_container_layout_hash(),
            ),
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(
                &agent,
                &Settings::default(),
                Some(&host_home),
                Some(&data_local_dir),
                None,
            )
            .unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to start Docker container"));
        assert!(message.contains("start failed"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_noops_when_running_without_home() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        let log = temp.path().join("docker.log");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;
        let name = container_name(&agent);

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  if printf '%s' \"$3\" | grep -q '.State.Running'; then\n    printf '%s\\n' 'true'\n  else\n    printf '%s\\n' '{}'\n  fi\n  exit 0\nfi\nexit 0\n",
                log.display(),
                default_worker_dockerfile_hash(),
                current_container_layout_hash(),
            ),
        );

        with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap();
        });

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(!log_contents.contains(&format!("start {name}")));
        assert!(!log_contents.contains(&format!("rm -f {name}")));
        assert!(!log_contents.contains("run -d --init"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_starts_container_when_not_running_without_home() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        let log = temp.path().join("docker.log");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  if printf '%s' \"$3\" | grep -q '.State.Running'; then\n    printf '%s\\n' 'false'\n  else\n    printf '%s\\n' '{}'\n  fi\n  exit 0\nfi\nif [ \"$1\" = \"start\" ]; then\n  exit 0\nfi\nexit 0\n",
                log.display(),
                default_worker_dockerfile_hash(),
                current_container_layout_hash(),
            ),
        );

        with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap();
        });

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains(&format!("start {}", container_name(&agent))));
        assert!(!log_contents.contains("run -d --init"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_reports_start_failure_without_home() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  if printf '%s' \"$3\" | grep -q '.State.Running'; then\n    printf '%s\\n' 'false'\n  else\n    printf '%s\\n' '{}'\n  fi\n  exit 0\nfi\nif [ \"$1\" = \"start\" ]; then\n  echo 'start failed' >&2\n  exit 1\nfi\nexit 0\n",
                default_worker_dockerfile_hash(),
                current_container_layout_hash(),
            ),
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to start Docker container"));
        assert!(message.contains("start failed"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_recreates_stale_container_when_not_running() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        let log = temp.path().join("docker.log");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;
        let name = container_name(&agent);

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  if printf '%s' \"$3\" | grep -q '.State.Running'; then\n    printf '%s\\n' 'false'\n  else\n    printf '%s\\n' 'stale-layout'\n  fi\n  exit 0\nfi\nif [ \"$1\" = \"rm\" ] || [ \"$1\" = \"run\" ]; then\n  exit 0\nfi\nexit 0\n",
                log.display(),
                default_worker_dockerfile_hash(),
            ),
        );

        with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap();
        });

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains(&format!("rm -f {name}")));
        assert!(log_contents.contains("run -d --init"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_skips_user_flag_when_unavailable() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        let log = temp.path().join("docker.log");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  printf '%s\\n' 'No such object' >&2\n  exit 1\nfi\nif [ \"$1\" = \"run\" ]; then\n  exit 0\nfi\nexit 0\n",
                log.display(),
                default_worker_dockerfile_hash(),
            ),
        );

        with_docker_program_override_for_tests(script, || {
            let _user_guard = set_docker_user_override_for_tests(None);
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap();
        });

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains("run -d --init"));
        assert!(!log_contents.contains("--user"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_propagates_remove_container_errors() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  if printf '%s' \"$3\" | grep -q '.State.Running'; then\n    printf '%s\\n' 'false'\n  else\n    printf '%s\\n' 'stale-layout'\n  fi\n  exit 0\nfi\nif [ \"$1\" = \"rm\" ]; then\n  printf '%s\\n' 'permission denied' >&2\n  exit 1\nfi\nexit 0\n",
                default_worker_dockerfile_hash(),
            ),
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap_err()
        });

        let message = err.to_string();
        assert!(message.contains("Failed to remove Docker container"));
        assert!(message.contains("permission denied"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_propagates_refresh_runtime_home_errors() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let data_local_dir = temp.path().join("xdg-data");
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&host_home).unwrap();
        fs::create_dir_all(&data_local_dir).unwrap();
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let runtime_root = runtime_root_dir(&agent, &data_local_dir);
        fs::create_dir_all(&runtime_root).unwrap();
        fs::write(runtime_root.join("home"), "not a directory").unwrap();

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  if printf '%s' \"$3\" | grep -q '.State.Running'; then\n    printf '%s\\n' 'true'\n  else\n    printf '%s\\n' '{}'\n  fi\n  exit 0\nfi\nexit 0\n",
                default_worker_dockerfile_hash(),
                current_container_layout_hash(),
            ),
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(
                &agent,
                &Settings::default(),
                Some(&host_home),
                Some(&data_local_dir),
                None,
            )
            .unwrap_err()
        });

        let message = err.to_string();
        assert!(message.contains("Failed to create Docker runtime cache directory"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_propagates_unexpected_inspect_errors() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  printf '%s\\n' 'boom' >&2\n  exit 1\nfi\nexit 0\n",
                default_worker_dockerfile_hash(),
            ),
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to inspect Docker container"));
        assert!(message.contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_treats_no_such_container_as_missing() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        let log = temp.path().join("docker.log");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  echo 'No such container' >&2\n  exit 1\nfi\nexit 0\n",
                log.display(),
                default_worker_dockerfile_hash(),
            ),
        );

        with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap();
        });

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains("run -d --init"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_propagates_prepare_runtime_home_errors_when_creating_container()
     {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let data_local_file = temp.path().join("data-local");
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&host_home).unwrap();
        fs::create_dir_all(&worktree).unwrap();
        fs::write(&data_local_file, "not-a-directory").unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  echo 'No such object' >&2\n  exit 1\nfi\nexit 0\n",
                default_worker_dockerfile_hash(),
            ),
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(
                &agent,
                &Settings::default(),
                Some(&host_home),
                Some(&data_local_file),
                None,
            )
            .unwrap_err()
        });
        assert!(
            err.to_string()
                .contains("Failed to create Docker runtime cache directory")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_reports_container_create_failures() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  echo 'No such object' >&2\n  exit 1\nfi\nif [ \"$1\" = \"run\" ]; then\n  echo 'run failed' >&2\n  exit 1\nfi\nexit 0\n",
                default_worker_dockerfile_hash(),
            ),
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to create Docker container"));
        assert!(message.contains("run failed"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_propagates_image_ready_errors() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  echo 'boom' >&2\n  exit 1\nfi\nexit 0\n",
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to inspect Docker image"));
        assert!(message.contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_propagates_layout_hash_errors() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&worktree).unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;

        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  case \"$3\" in\n    *State.Running*)\n      printf '%s\\n' 'true'\n      exit 0\n      ;;\n    *)\n      echo 'boom' >&2\n      exit 1\n      ;;\n  esac\nfi\nexit 0\n",
                default_worker_dockerfile_hash(),
            ),
        );

        let err = with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(&agent, &Settings::default(), None, None, None).unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to inspect Docker container"));
        assert!(message.contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_container_by_name_ignores_missing_container_errors() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"rm\" ]; then\n  printf '%s\\n' 'No such container' >&2\n  exit 1\nfi\nexit 0\n",
        );

        let result = with_docker_program_override_for_tests(script, || {
            remove_container_by_name("tenex-missing")
        });
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_container_by_name_ignores_missing_object_errors() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"rm\" ]; then\n  printf '%s\\n' 'No such object' >&2\n  exit 1\nfi\nexit 0\n",
        );

        let result = with_docker_program_override_for_tests(script, || {
            remove_container_by_name("tenex-missing")
        });
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_container_by_name_propagates_unexpected_errors() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(
            &temp,
            "#!/bin/sh\nset -eu\nif [ \"$1\" = \"rm\" ]; then\n  printf '%s\\n' 'permission denied' >&2\n  exit 1\nfi\nexit 0\n",
        );

        let err = with_docker_program_override_for_tests(script, || {
            remove_container_by_name("tenex-missing").unwrap_err()
        });
        let message = err.to_string();
        assert!(message.contains("Failed to remove Docker container"));
        assert!(message.contains("permission denied"));
    }

    #[cfg(unix)]
    #[test]
    fn test_prepare_runtime_home_in_stages_writable_ssh_home() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("home");
        let data_local_dir = temp.path().join("data");
        let host_codex_home = host_home.join(".codex");
        let host_ssh_dir = host_home.join(".ssh");
        let host_xdg_ssh_dir = host_home.join(".config").join("ssh");
        let external_key = temp.path().join("id_test");
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();
        fs::create_dir_all(&host_ssh_dir).unwrap();
        fs::create_dir_all(&host_xdg_ssh_dir).unwrap();
        fs::write(host_ssh_dir.join("config"), "Host test\n").unwrap();
        fs::write(host_ssh_dir.join("known_hosts"), "host-key\n").unwrap();
        fs::write(&external_key, "private-key\n").unwrap();
        std::os::unix::fs::symlink(&external_key, host_ssh_dir.join("id_test")).unwrap();
        fs::write(host_xdg_ssh_dir.join("config"), "Host xdg-test\n").unwrap();
        fs::set_permissions(&host_ssh_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(
            host_ssh_dir.join("config"),
            std::fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        fs::set_permissions(
            host_ssh_dir.join("known_hosts"),
            std::fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        fs::set_permissions(&host_xdg_ssh_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(
            host_xdg_ssh_dir.join("config"),
            std::fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        fs::set_permissions(&external_key, std::fs::Permissions::from_mode(0o400)).unwrap();

        let prepared = prepare_runtime_home_in(
            &docker_agent(),
            &host_home,
            &data_local_dir,
            &host_codex_home,
        )
        .unwrap();

        let staged_ssh_dir = prepared.home_source.join(".ssh");
        let staged_xdg_ssh_dir = prepared.home_source.join(".config").join("ssh");
        assert_eq!(
            fs::read_to_string(staged_ssh_dir.join("config")).unwrap(),
            "Host test\n"
        );
        assert_eq!(
            fs::read_to_string(staged_ssh_dir.join("id_test")).unwrap(),
            "private-key\n"
        );
        assert_eq!(
            fs::read_to_string(staged_xdg_ssh_dir.join("config")).unwrap(),
            "Host xdg-test\n"
        );

        fs::write(staged_ssh_dir.join("known_hosts"), "updated-host-key\n").unwrap();
        fs::write(staged_ssh_dir.join("control-socket"), "socket\n").unwrap();
        fs::write(staged_xdg_ssh_dir.join("control-socket"), "socket\n").unwrap();
        assert_ne!(
            fs::metadata(&staged_ssh_dir).unwrap().permissions().mode() & 0o200,
            0
        );
        assert_ne!(
            fs::metadata(staged_ssh_dir.join("config"))
                .unwrap()
                .permissions()
                .mode()
                & 0o200,
            0
        );
        assert_eq!(
            fs::read_to_string(host_ssh_dir.join("known_hosts")).unwrap(),
            "host-key\n"
        );
        fs::set_permissions(&host_ssh_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&host_xdg_ssh_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_with_paths_mounts_optional_host_config() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let data_local_dir = temp.path().join("xdg-data");
        let worktree = temp.path().join("worktree");
        let host_codex_home = host_home.join(".codex");
        let host_gh_dir = host_home.join(".config").join("gh");
        let log = temp.path().join("docker.log");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(host_codex_home.join("sessions")).unwrap();
        fs::create_dir_all(host_codex_home.join("skills")).unwrap();
        fs::create_dir_all(host_home.join(".ssh")).unwrap();
        fs::create_dir_all(host_home.join(".config").join("ssh")).unwrap();
        fs::create_dir_all(&host_gh_dir).unwrap();
        fs::write(host_codex_home.join("config.toml"), "model = \"gpt-5.4\"\n").unwrap();
        fs::write(host_home.join(".ssh").join("config"), "Host test\n").unwrap();
        fs::write(
            host_home.join(".config").join("ssh").join("config"),
            "Host xdg-test\n",
        )
        .unwrap();
        fs::write(host_home.join(".gitconfig"), "[user]\n\tname = Test User\n").unwrap();
        fs::write(host_gh_dir.join("hosts.yml"), "github.com:\n").unwrap();

        let mut agent = docker_agent();
        agent.worktree_path = worktree;
        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  echo 'No such object' >&2\n  exit 1\nfi\nif [ \"$1\" = \"run\" ] || [ \"$1\" = \"rm\" ]; then\n  exit 0\nfi\nexit 0\n",
                log.display(),
                default_worker_dockerfile_hash(),
            ),
        );

        with_docker_program_override_for_tests(script, || {
            ensure_container_with_paths(
                &agent,
                &Settings::default(),
                Some(&host_home),
                Some(&data_local_dir),
                None,
            )
            .unwrap();
        });

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains("run -d --init"));
        assert!(log_contents.contains(&format!(
            "-v {}:{}",
            host_gh_dir.display(),
            host_gh_dir.display()
        )));
        assert!(log_contents.contains(&format!(
            "-v {}:{}:ro",
            host_home.join(".gitconfig").display(),
            host_home.join(".gitconfig").display()
        )));
        assert!(log_contents.contains(&format!(
            "-v {}:{}:ro",
            host_codex_home.join("skills").display(),
            host_codex_home.join("skills").display()
        )));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_uses_process_environment() {
        if std::env::var_os(ENSURE_CONTAINER_CHILD_FLAG).is_some() {
            let mut agent = docker_agent();
            agent.worktree_path =
                PathBuf::from(std::env::var_os(ENSURE_CONTAINER_WORKTREE_VAR).unwrap());
            ensure_container(&agent, &Settings::default()).unwrap();
            remove_container(&agent).unwrap();
            return;
        }

        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let data_local_dir = temp.path().join("xdg-data");
        let worktree = temp.path().join("worktree");
        let log = temp.path().join("docker.log");
        let path = std::env::var("PATH").unwrap_or_default();
        let prefixed_path = format!("{}:{path}", temp.path().display());
        fs::create_dir_all(home.join(".codex").join("sessions")).unwrap();
        fs::create_dir_all(home.join(".ssh")).unwrap();
        fs::create_dir_all(home.join(".config").join("ssh")).unwrap();
        fs::create_dir_all(&worktree).unwrap();
        fs::write(
            home.join(".codex").join("config.toml"),
            "model = \"gpt-5.4\"\n",
        )
        .unwrap();
        fs::write(home.join(".ssh").join("config"), "Host test\n").unwrap();
        fs::write(
            home.join(".config").join("ssh").join("config"),
            "Host xdg-test\n",
        )
        .unwrap();
        let _ = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  echo 'No such object' >&2\n  exit 1\nfi\nif [ \"$1\" = \"run\" ] || [ \"$1\" = \"rm\" ]; then\n  exit 0\nfi\nexit 0\n",
                log.display(),
                default_worker_dockerfile_hash(),
            ),
        );

        let current_exe = std::env::current_exe().unwrap();
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg("runtime::docker::tests::test_ensure_container_uses_process_environment")
            .arg("--nocapture")
            .env(ENSURE_CONTAINER_CHILD_FLAG, "1")
            .env(ENSURE_CONTAINER_WORKTREE_VAR, &worktree)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", &data_local_dir)
            .env("PATH", prefixed_path)
            .output()
            .unwrap();

        assert!(output.status.success());
        assert!(log.exists());

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains("run -d --init"));
        assert!(log_contents.contains(&format!("HOME={}", home.display())));
        assert!(log_contents.contains(&format!(
            "-v {}:{}",
            worktree.display(),
            worktree.display()
        )));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_mounts_runtime_env_and_symlink_targets() {
        let temp = TempDir::new().unwrap();
        let (log_contents, prepared, repo_root, host_home) = docker_mount_log_for_test(&temp);
        let expected_target_mount = repo_root.join("target").canonicalize().unwrap();
        let expected_plan_mount = repo_root.join("PLAN.md").canonicalize().unwrap();
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
        assert!(log_contents.contains(&format!("LD_PRELOAD={NSS_WRAPPER_LIB_PATH}")));
        assert!(log_contents.contains(&format!(
            "NSS_WRAPPER_PASSWD={}",
            host_home.join(RUNTIME_PASSWD_FILE_NAME).display()
        )));
        assert!(log_contents.contains(&format!(
            "NSS_WRAPPER_GROUP={}",
            host_home.join(RUNTIME_GROUP_FILE_NAME).display()
        )));
        assert!(prepared.home_source.join(".ssh").join("config").is_file());
        assert!(
            prepared
                .home_source
                .join(".config")
                .join("ssh")
                .join("config")
                .is_file()
        );
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
        assert!(!log_contents.contains(&format!(
            "-v {}:{}",
            host_home.join(".ssh").display(),
            host_home.join(".ssh").display()
        )));
        assert!(!log_contents.contains(&format!(
            "-v {}:{}",
            host_home.join(".config").join("ssh").display(),
            host_home.join(".config").join("ssh").display()
        )));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_mount_log_omits_user_arg_when_user_unavailable() {
        let temp = TempDir::new().unwrap();
        let _guard = set_docker_user_override_for_tests(None);

        let (log_contents, ..) = docker_mount_log_for_test(&temp);

        assert!(!log_contents.contains("--user"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_mount_log_omits_identity_env_when_user_info_unavailable() {
        if std::env::var_os(MOUNT_LOG_IDENTITY_CHILD_FLAG).is_some() {
            let temp = TempDir::new().unwrap();
            let (log_contents, ..) = docker_mount_log_for_test(&temp);

            assert!(!log_contents.contains("USER="));
            assert!(!log_contents.contains("LOGNAME="));
            return;
        }

        let current_exe = std::env::current_exe().unwrap();
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg("runtime::docker::tests::test_ensure_container_mount_log_omits_identity_env_when_user_info_unavailable")
            .arg("--nocapture")
            .env(MOUNT_LOG_IDENTITY_CHILD_FLAG, "1")
            .env("PATH", "")
            .output()
            .unwrap();

        assert!(output.status.success());
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_container_rejects_running_stale_layout() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let data_local_dir = temp.path().join("xdg-data");
        fs::create_dir_all(host_home.join(".codex").join("sessions")).unwrap();
        fs::create_dir_all(host_home.join(".claude")).unwrap();
        fs::write(
            host_home.join(".codex").join("config.toml"),
            "model = \"gpt-5.4\"\n",
        )
        .unwrap();
        fs::write(
            host_home.join(".claude").join("settings.json"),
            "{\"permissions\":{\"defaultMode\":\"plan\"}}",
        )
        .unwrap();
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
        );

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
    }

    #[cfg(unix)]
    struct DockerMountTestFixture {
        prepared: PreparedRuntimeHome,
        repo_root: PathBuf,
        worktree: PathBuf,
        host_home: PathBuf,
        agent: Agent,
    }

    #[cfg(unix)]
    fn prepare_docker_mount_test_fixture(temp: &TempDir) -> DockerMountTestFixture {
        let repo_root = temp.path().join("repo");
        let worktree = temp.path().join("worktree");
        let host_home = temp.path().join("host-home");
        let data_local_dir = temp.path().join("xdg-data");
        fs::create_dir_all(repo_root.join("target")).unwrap();
        fs::create_dir_all(
            repo_root
                .join(".git")
                .join("worktrees")
                .join("agent-docker"),
        )
        .unwrap();
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(host_home.join(".codex").join("sessions")).unwrap();
        fs::create_dir_all(host_home.join(".claude")).unwrap();
        fs::create_dir_all(host_home.join(".ssh")).unwrap();
        fs::create_dir_all(host_home.join(".config").join("ssh")).unwrap();
        fs::write(repo_root.join("PLAN.md"), "# plan\n").unwrap();
        fs::write(host_home.join(".ssh").join("config"), "# test ssh config\n").unwrap();
        fs::write(
            host_home.join(".config").join("ssh").join("config"),
            "# test xdg ssh config\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(repo_root.join("target"), worktree.join("target")).unwrap();
        std::os::unix::fs::symlink(repo_root.join("PLAN.md"), worktree.join("PLAN.md")).unwrap();
        fs::write(
            host_home.join(".codex").join("config.toml"),
            "model = \"gpt-5.4\"\n",
        )
        .unwrap();
        fs::write(
            host_home.join(".claude").join("settings.json"),
            "{\"hooks\":{\"Stop\":[]}}",
        )
        .unwrap();

        let mut agent = docker_agent();
        agent.repo_root = Some(repo_root.clone());
        agent.worktree_path = worktree.clone();
        let prepared = prepare_runtime_home_in(
            &agent,
            &host_home,
            &data_local_dir,
            &host_home.join(".codex"),
        )
        .unwrap();

        DockerMountTestFixture {
            prepared,
            repo_root,
            worktree,
            host_home,
            agent,
        }
    }

    #[cfg(unix)]
    fn create_test_container_mount_log(script: PathBuf, fixture: &DockerMountTestFixture) {
        with_docker_program_override_for_tests(script, || {
            let settings = Settings::default();
            let image = worker_image_tag(&settings);
            let mut cmd = docker_command();
            cmd.args([
                "run",
                "-d",
                "--init",
                "--name",
                &container_name(&fixture.agent),
                "--hostname",
                &container_name(&fixture.agent),
            ]);
            if let Some(user) = docker_user_arg() {
                cmd.args(["--user", &user]);
            }
            cmd.arg("--label").arg(format!(
                "{WORKER_CONTAINER_LAYOUT_HASH_LABEL}={}",
                current_container_layout_hash()
            ));
            cmd.arg("-e")
                .arg(format!("HOME={}", fixture.host_home.display()));
            cmd.arg("-e").arg(format!(
                "XDG_CACHE_HOME={}",
                fixture.host_home.join(".cache").display()
            ));
            cmd.arg("-e").arg(format!(
                "CARGO_HOME={}",
                fixture.host_home.join(".cargo").display()
            ));
            cmd.arg("-e").arg(format!(
                "CODEX_HOME={}",
                fixture.host_home.join(".codex").display()
            ));
            cmd.arg("-e")
                .arg(format!("LD_PRELOAD={NSS_WRAPPER_LIB_PATH}"));
            cmd.arg("-e").arg(format!(
                "NSS_WRAPPER_PASSWD={}",
                fixture.host_home.join(RUNTIME_PASSWD_FILE_NAME).display()
            ));
            cmd.arg("-e").arg(format!(
                "NSS_WRAPPER_GROUP={}",
                fixture.host_home.join(RUNTIME_GROUP_FILE_NAME).display()
            ));
            if let Some(identity) = current_runtime_user_info() {
                cmd.arg("-e").arg(format!("USER={}", identity.user_name));
                cmd.arg("-e").arg(format!("LOGNAME={}", identity.user_name));
            }
            add_bind_mount(
                &mut cmd,
                &fixture.prepared.home_source,
                &fixture.host_home,
                false,
            );
            add_bind_mount(
                &mut cmd,
                &fixture.prepared.codex_home_source,
                &fixture.prepared.codex_home_target,
                false,
            );
            add_bind_mount(
                &mut cmd,
                &fixture.prepared.codex_home_target.join("sessions"),
                &fixture.prepared.codex_home_target.join("sessions"),
                false,
            );
            configure_repo_metadata_mounts(&mut cmd, &fixture.agent);
            cmd.args(["-w", &display_path(&fixture.worktree)]);
            add_bind_mount(&mut cmd, &fixture.worktree, &fixture.worktree, false);
            cmd.arg(image);
            cmd.arg("sleep");
            cmd.arg("infinity");
            run_command(&mut cmd, "Failed to create Docker container")
        })
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_configure_top_level_symlink_mounts_returns_early_when_read_dir_fails() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        fs::write(&worktree, "not-a-dir").unwrap();
        let mut cmd = Command::new("docker");
        let mut mounted_targets = std::collections::HashSet::new();

        configure_top_level_symlink_mounts(&mut cmd, &worktree, &mut mounted_targets);

        assert!(cmd.get_args().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_configure_top_level_symlink_mounts_for_paths_falls_back_when_worktree_canonicalize_fails()
     {
        let temp = TempDir::new().unwrap();
        let missing_worktree = temp.path().join("missing-worktree");
        let mut cmd = Command::new("docker");
        let mut mounted_targets = std::collections::HashSet::new();

        configure_top_level_symlink_mounts_for_paths(
            &mut cmd,
            &missing_worktree,
            &mut mounted_targets,
            Vec::new(),
        );

        assert!(cmd.get_args().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_configure_top_level_symlink_mounts_for_paths_skips_missing_metadata() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&worktree).unwrap();
        let missing = worktree.join("missing");
        let mut cmd = Command::new("docker");
        let mut mounted_targets = std::collections::HashSet::new();

        configure_top_level_symlink_mounts_for_paths(
            &mut cmd,
            &worktree,
            &mut mounted_targets,
            vec![missing],
        );

        assert!(cmd.get_args().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_configure_top_level_symlink_mounts_for_paths_skips_broken_symlink() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&worktree).unwrap();
        let broken = worktree.join("broken");
        std::os::unix::fs::symlink(temp.path().join("missing-target"), &broken).unwrap();
        let mut cmd = Command::new("docker");
        let mut mounted_targets = std::collections::HashSet::new();

        configure_top_level_symlink_mounts_for_paths(
            &mut cmd,
            &worktree,
            &mut mounted_targets,
            vec![broken],
        );

        assert!(cmd.get_args().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_configure_top_level_symlink_mounts_for_paths_skips_symlinks_resolving_inside_worktree()
    {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        fs::create_dir_all(&worktree).unwrap();
        let target = worktree.join("target");
        fs::create_dir_all(&target).unwrap();
        let link = worktree.join("target-link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let mut cmd = Command::new("docker");
        let mut mounted_targets = std::collections::HashSet::new();

        configure_top_level_symlink_mounts_for_paths(
            &mut cmd,
            &worktree,
            &mut mounted_targets,
            vec![link],
        );

        assert!(cmd.get_args().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_configure_top_level_symlink_mounts_for_paths_mounts_symlink_targets_outside_worktree() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        let external = temp.path().join("external");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(&external).unwrap();
        let external_target = external.canonicalize().unwrap();
        let link = worktree.join("external-link");
        std::os::unix::fs::symlink(&external_target, &link).unwrap();
        let mut cmd = Command::new("docker");
        let mut mounted_targets = std::collections::HashSet::new();

        configure_top_level_symlink_mounts_for_paths(
            &mut cmd,
            &worktree,
            &mut mounted_targets,
            vec![link],
        );

        let args = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.contains(&"-v".to_string()));
        assert!(args.iter().any(|arg| {
            arg == &format!(
                "{}:{}",
                external_target.display(),
                external_target.display()
            )
        }));
    }

    #[cfg(unix)]
    #[test]
    fn test_resolved_symlink_target_accepts_absolute_targets() {
        let temp = TempDir::new().unwrap();
        let worktree = temp.path().join("worktree");
        let external = temp.path().join("external");
        let link_path = worktree.join("PLAN.md");
        let target = external.join("PLAN.md");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(&external).unwrap();
        fs::write(&target, "# plan\n").unwrap();

        let resolved = resolved_symlink_target(&link_path, &worktree, &target);

        assert_eq!(resolved, target.canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_optional_file_reports_parent_creation_failures() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source.txt");
        fs::write(&source, "data").unwrap();
        let parent = temp.path().join("parent");
        fs::write(&parent, "not-a-dir").unwrap();
        let target = parent.join("child.txt");

        let err =
            sync_optional_file(&source, &target).expect_err("expected parent creation failure");
        let message = err.to_string();
        assert!(message.contains("Failed to create"));
        assert!(message.contains(&parent.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_optional_file_handles_root_target_parent_none() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source.txt");
        fs::write(&source, "data").unwrap();

        let err =
            sync_optional_file(&source, Path::new("/")).expect_err("expected root copy failure");
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_copies_dirs_files_and_symlinks() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let nested_dir = source.join("nested");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(source.join("root.txt"), "root\n").unwrap();
        fs::write(nested_dir.join("nested.txt"), "nested\n").unwrap();
        UnixListener::bind(source.join("socket")).unwrap();
        std::os::unix::fs::symlink(source.join("root.txt"), source.join("root-link")).unwrap();
        std::os::unix::fs::symlink(&nested_dir, source.join("nested-link")).unwrap();

        copy_path_recursive_following_symlinks(&source, &target).unwrap();

        assert_eq!(
            fs::read_to_string(target.join("root.txt")).unwrap(),
            "root\n"
        );
        assert_eq!(
            fs::read_to_string(target.join("root-link")).unwrap(),
            "root\n"
        );
        assert_eq!(
            fs::read_to_string(target.join("nested").join("nested.txt")).unwrap(),
            "nested\n"
        );
        assert_eq!(
            fs::read_to_string(target.join("nested-link").join("nested.txt")).unwrap(),
            "nested\n"
        );
        assert!(!target.join("socket").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_skips_symlink_loops() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        std::os::unix::fs::symlink(&source, source.join("self")).unwrap();

        copy_path_recursive_following_symlinks(&source, &target).unwrap();

        assert!(!target.join("self").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_propagates_failures_from_symlink_targets() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let external = temp.path().join("external");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("secret.txt"), "data").unwrap();
        std::os::unix::fs::symlink(&external, source.join("external-link")).unwrap();

        fs::create_dir_all(target.join("external-link").join("secret.txt")).unwrap();

        let err = copy_path_recursive_following_symlinks(&source, &target)
            .expect_err("expected symlink copy failure");
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_propagates_failures_from_dir_entries() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let nested = source.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("file.txt"), "data").unwrap();

        fs::create_dir_all(target.join("nested").join("file.txt")).unwrap();

        let err =
            copy_path_recursive_following_symlinks(&source, &target).expect_err("expected failure");
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_reports_dir_entry_read_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("file.txt"), "data").unwrap();

        let err = with_copy_path_recursive_following_symlinks_dir_entry_hook(
            |_| Err(std::io::Error::other("boom")),
            || copy_path_recursive_following_symlinks(&source, &target).unwrap_err(),
        );
        let message = err.to_string();
        let message_with_causes = format!("{err:#}");
        assert!(message.contains("Failed to read"));
        assert!(message_with_causes.contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_ssh_home_propagates_failures_when_copying_dot_ssh() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let target_home = temp.path().join("target-home");
        fs::create_dir_all(host_home.join(".ssh")).unwrap();
        fs::create_dir_all(&target_home).unwrap();
        fs::write(host_home.join(".ssh").join("config"), "Host test\n").unwrap();

        let err = with_copy_path_recursive_before_symlink_metadata_hook(
            |path| {
                let _ = fs::remove_file(path);
            },
            || sync_ssh_home(&target_home, &host_home).unwrap_err(),
        );

        assert!(err.to_string().contains("Failed to read file type for"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_ssh_home_propagates_failures_when_copying_xdg_ssh() {
        let temp = TempDir::new().unwrap();
        let host_home = temp.path().join("host-home");
        let target_home = temp.path().join("target-home");
        fs::create_dir_all(host_home.join(".config").join("ssh")).unwrap();
        fs::create_dir_all(&target_home).unwrap();
        fs::write(
            host_home.join(".config").join("ssh").join("config"),
            "Host xdg-test\n",
        )
        .unwrap();

        let err = with_copy_path_recursive_before_symlink_metadata_hook(
            |path| {
                let _ = fs::remove_file(path);
            },
            || sync_ssh_home(&target_home, &host_home).unwrap_err(),
        );

        assert!(err.to_string().contains("Failed to read file type for"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_optional_path_following_symlinks_propagates_remove_errors() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("missing-source");
        let target = temp.path().join("unremovable");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("file.txt"), "hello").unwrap();

        let mut perms = fs::metadata(&target).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&target, perms).unwrap();

        let err = sync_optional_path_following_symlinks(&source, &target).unwrap_err();
        assert!(err.to_string().contains("Failed to remove"));

        let mut reset = fs::metadata(&target).unwrap().permissions();
        reset.set_mode(0o755);
        fs::set_permissions(&target, reset).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_file_with_permissions_handles_root_target_parent_none() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source.txt");
        fs::write(&source, "data").unwrap();

        let err = copy_file_with_permissions(&source, Path::new("/"))
            .expect_err("expected root target copy failure");
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_path_recursive_following_symlinks_noops_for_sockets() {
        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("socket");
        UnixListener::bind(&socket_path).unwrap();
        let target = temp.path().join("socket-copy");

        copy_path_recursive_following_symlinks(&socket_path, &target).unwrap();

        assert!(!target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_dir_recursive_skips_symlinks() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("file.txt"), "data").unwrap();
        std::os::unix::fs::symlink(source.join("file.txt"), source.join("file-link")).unwrap();

        copy_dir_recursive(&source, &target).unwrap();

        assert_eq!(fs::read_to_string(target.join("file.txt")).unwrap(), "data");
        assert!(!target.join("file-link").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_dir_recursive_reports_copy_failures_with_context() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(source.join("file.txt"), "data").unwrap();
        fs::create_dir_all(target.join("file.txt")).unwrap();

        let err = copy_dir_recursive(&source, &target).expect_err("expected copy failure");
        let message = err.to_string();
        assert!(message.contains("Failed to copy"));
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_claude_settings_file_removes_target_when_source_missing() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("missing-source");
        let target = temp.path().join("settings.json");
        fs::write(&target, "{}").unwrap();

        sync_claude_settings_file(&source, &target).unwrap();

        assert!(!target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_write_runtime_identity_files_clear_root_entries_when_uid_gid_zero() {
        if std::env::var_os(RUNTIME_IDENTITY_CHILD_FLAG).is_some() {
            let home_source =
                PathBuf::from(std::env::var_os(RUNTIME_IDENTITY_HOME_SOURCE_VAR).unwrap());
            let home_target = PathBuf::from("/tmp/runtime-home");
            write_runtime_identity_files(&home_source, &home_target).unwrap();
            let passwd = fs::read_to_string(home_source.join(RUNTIME_PASSWD_FILE_NAME)).unwrap();
            let group = fs::read_to_string(home_source.join(RUNTIME_GROUP_FILE_NAME)).unwrap();
            assert!(!passwd.contains("root:x:0:0:root:/root:/bin/bash"));
            assert!(!group.contains("root:x:0:\n"));
            assert!(passwd.contains("Tenex runtime user"));
            return;
        }

        let temp = TempDir::new().unwrap();
        let home_source = temp.path().join("runtime-home");
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&home_source).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();

        let id_script = bin_dir.join("id");
        fs::write(
            &id_script,
            "#!/bin/sh\ncase \"$1\" in\n  -u|-g) echo 0 ;;\n  -un|-gn) echo tenex ;;\n  *) exit 1 ;;\nesac\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&id_script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&id_script, perms).unwrap();

        let current_exe = std::env::current_exe().unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        let prefixed_path = format!("{}:{path}", bin_dir.display());
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg("runtime::docker::tests::test_write_runtime_identity_files_clear_root_entries_when_uid_gid_zero")
            .arg("--nocapture")
            .env(RUNTIME_IDENTITY_CHILD_FLAG, "1")
            .env(RUNTIME_IDENTITY_HOME_SOURCE_VAR, &home_source)
            .env("PATH", prefixed_path)
            .output()
            .unwrap();

        assert!(output.status.success());
    }

    #[cfg(unix)]
    fn docker_mount_log_for_test(
        temp: &TempDir,
    ) -> (String, PreparedRuntimeHome, PathBuf, PathBuf) {
        let log = temp.path().join("docker.log");
        let script = write_fake_docker_script(
            temp,
            &format!(
                "#!/bin/sh\nset -eu\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ]; then\n  echo 'No such object' >&2\n  exit 1\nfi\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
                log.display()
            ),
        );

        let fixture = prepare_docker_mount_test_fixture(temp);
        create_test_container_mount_log(script, &fixture);

        (
            fs::read_to_string(&log).unwrap(),
            fixture.prepared,
            fixture.repo_root,
            fixture.host_home,
        )
    }
}
