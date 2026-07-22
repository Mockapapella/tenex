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

const DEFAULT_DOCKER_IMAGE: &str = "tenex-worker:latest";
const DEFAULT_WORKER_DOCKERFILE_TEMPLATE: &str = include_str!("../../docker/worker.Dockerfile");
const RUNTIME_HOME_ROOT_DIR: &str = "docker-runtime";
const WORKER_IMAGE_TEMPLATE_HASH_LABEL: &str = "dev.tenex.worker-template-fnv1a64";
const WORKER_CONTAINER_LAYOUT_HASH_LABEL: &str = "dev.tenex.runtime-template-fnv1a64";
const WORKER_CONTAINER_LAYOUT_VERSION: &str = "5";
const NSS_WRAPPER_LIB_PATH: &str = "/usr/local/lib/libnss_wrapper.so";
const RUNTIME_PASSWD_FILE_NAME: &str = ".tenex-passwd";
const RUNTIME_GROUP_FILE_NAME: &str = ".tenex-group";
#[cfg(unix)]
const DEFAULT_RUNTIME_USER_NAME: &str = "tenex";
#[cfg(unix)]
const DEFAULT_RUNTIME_GROUP_NAME: &str = "tenex";
#[cfg(windows)]
const WINDOWS_CONTAINER_ROOT: &str = "/tenex-host";

struct PreparedRuntimeHome {
    home_source: PathBuf,
    codex_home_source: PathBuf,
    codex_home_target: PathBuf,
}

#[derive(Default)]
struct DockerHostPaths {
    runtime_home: Option<DockerHostHome>,
    ssh_auth_sock: Option<PathBuf>,
}

struct DockerHostHome {
    home: PathBuf,
    data_local_dir: PathBuf,
    codex_home: PathBuf,
}

impl DockerHostPaths {
    fn from_process_environment() -> Self {
        let runtime_home = paths::home_dir().map(|home| DockerHostHome {
            data_local_dir: paths::data_local_dir().unwrap_or_else(|| home.clone()),
            codex_home: codex_home_dir(&home),
            home,
        });

        Self {
            runtime_home,
            ssh_auth_sock: std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from),
        }
    }
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
    let host_paths = DockerHostPaths::from_process_environment();
    ensure_container_with_paths(agent, settings, &host_paths)
}

fn ensure_container_with_paths(
    agent: &Agent,
    settings: &Settings,
    host_paths: &DockerHostPaths,
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
            } else if let Some(runtime_home) = host_paths.runtime_home.as_ref() {
                refresh_runtime_home_for_reuse_in(
                    agent,
                    &runtime_home.home,
                    &runtime_home.data_local_dir,
                    &runtime_home.codex_home,
                )?;
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

    if let Some(runtime_home) = host_paths.runtime_home.as_ref() {
        configure_home_mounts(&mut cmd, agent, runtime_home)?;
    }

    configure_ssh_auth_sock_mount_from(&mut cmd, host_paths.ssh_auth_sock.as_deref());
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

fn container_name(agent: &Agent) -> String {
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

#[cfg(windows)]
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

fn configure_home_mounts(
    cmd: &mut Command,
    agent: &Agent,
    runtime_home: &DockerHostHome,
) -> Result<()> {
    let home = &runtime_home.home;
    let prepared_home = prepare_runtime_home_in(
        agent,
        home,
        &runtime_home.data_local_dir,
        &runtime_home.codex_home,
    )?;
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
            let entry = entry.context(format!("Failed to read {}", source.display()))?;
            let entry_path = entry.path();

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

    let permissions = std::fs::metadata(source)
        .with_context(|| format!("Failed to read {}", source.display()))?
        .permissions();
    set_staged_permissions(target, permissions)?;
    Ok(())
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
        let entry = entry.context(format!("Failed to read {}", source.display()))?;
        let entry_path = entry.path();

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

#[cfg(unix)]
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

    let output = wait_with_output_for_docker_build(|| child.wait_with_output(), &program, &args)?;

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
