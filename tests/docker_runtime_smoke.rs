//! Integration smoke coverage for the Docker root-agent path.

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::ErrorKind;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::sync::OnceLock;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use git2::{RepositoryInitOptions, Signature};
#[cfg(unix)]
use tempfile::TempDir;

#[cfg(unix)]
const CHILD_FLAG: &str = "TENEX_DOCKER_RUNTIME_SMOKE_CHILD";
#[cfg(unix)]
const STATE_PATH_VAR: &str = "TENEX_DOCKER_RUNTIME_SMOKE_STATE_PATH";
#[cfg(unix)]
const PLAIN_DIR_VAR: &str = "TENEX_DOCKER_RUNTIME_SMOKE_PLAIN_DIR";
#[cfg(unix)]
const LOG_PATH_VAR: &str = "TENEX_DOCKER_RUNTIME_SMOKE_LOG_PATH";
#[cfg(unix)]
const GIT_REPO_DIR_VAR: &str = "TENEX_DOCKER_RUNTIME_SMOKE_GIT_REPO_DIR";
#[cfg(unix)]
const WORKTREE_ROOT_VAR: &str = "TENEX_DOCKER_RUNTIME_SMOKE_WORKTREE_ROOT";
#[cfg(unix)]
const GIT_ROOT_CHILD_FLAG: &str = "TENEX_DOCKER_RUNTIME_SMOKE_GIT_ROOT_CHILD";

#[cfg(unix)]
const WORKER_DOCKERFILE_TEMPLATE: &str = include_str!("../docker/worker.Dockerfile");
#[cfg(unix)]
const WORKER_CONTAINER_LAYOUT_VERSION: &str = "5";

#[cfg(unix)]
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(unix)]
fn current_container_layout_hash() -> String {
    let dockerfile_hash = format!("{:016x}", fnv1a64(WORKER_DOCKERFILE_TEMPLATE.as_bytes()));
    let descriptor = format!(
        "layout:{WORKER_CONTAINER_LAYOUT_VERSION};image:{dockerfile_hash};cargo-home:.cargo;mounts:repo-git,repo-worktrees,external-symlink-targets,managed-ssh-home;nss-wrapper"
    );
    format!("{:016x}", fnv1a64(descriptor.as_bytes()))
}

#[cfg(unix)]
fn skip_if_no_mux() -> bool {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let name = format!("tenex-mux-test-docker-runtime-{}", std::process::id());
        let _ = tenex::mux::set_socket_override(&name);
    });

    if !tenex::mux::is_available() {
        eprintln!("Skipping test: mux not available");
        return true;
    }
    false
}

fn write_fake_docker_script(dir: &Path, log: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let script = dir.join("docker");
    let state = dir.join("docker-state");
    fs::write(
        &script,
        format!(
            "#!/bin/sh
set -eu
printf '%s\\n' \"$*\" >> \"{log}\"
state_file='{state}'
if [ ! -f \"$state_file\" ]; then
  printf '%s' 'missing' > \"$state_file\"
fi
if [ \"$1\" = \"version\" ]; then
  exit 0
fi
if [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then
  echo 'No such image' >&2
  exit 1
fi
if [ \"$1\" = \"build\" ]; then
  cat >/dev/null
  exit 0
fi
if [ \"$1\" = \"inspect\" ]; then
  state=$(cat \"$state_file\")
  if printf '%s' \"$3\" | grep -q '.State.Running'; then
    if [ \"$state\" = 'missing' ]; then
      echo 'No such object' >&2
      exit 1
    fi
    if [ \"$state\" = 'running' ]; then
      echo 'true'
    else
      echo 'false'
    fi
    exit 0
  fi
  if [ \"$state\" = 'missing' ]; then
    echo 'No such object' >&2
    exit 1
  fi
  printf '%s\\n' '{layout_hash}'
  exit 0
fi
if [ \"$1\" = \"run\" ]; then
  printf '%s' 'stopped' > \"$state_file\"
  exit 0
fi
if [ \"$1\" = \"exec\" ]; then
  sleep 3600
  exit 0
fi
if [ \"$1\" = \"start\" ]; then
  printf '%s' 'running' > \"$state_file\"
  exit 0
fi
if [ \"$1\" = \"rm\" ]; then
  printf '%s' 'missing' > \"$state_file\"
  exit 0
fi
exit 0
",
            log = log.display(),
            state = state.display(),
            layout_hash = current_container_layout_hash(),
        ),
    )?;
    let mut perms = fs::metadata(&script)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms)?;
    Ok(script)
}

#[cfg(unix)]
fn wait_for_log_line(log: &Path, needle: &str) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if fs::read_to_string(log)
            .ok()
            .is_some_and(|contents| contents.contains(needle))
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    Err(format!("Timed out waiting for `{needle}` in {}", log.display()).into())
}

#[cfg(unix)]
fn required_env_path(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(std::env::var_os(name).ok_or_else(|| {
        std::io::Error::other(format!("Missing required env var {name}"))
    })?))
}

#[cfg(unix)]
fn first_path_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = std::env::var("PATH")
        .map_err(|_| std::io::Error::other("Missing PATH for docker runtime smoke child"))?;
    let first = path
        .split(':')
        .find(|segment| !segment.is_empty())
        .ok_or_else(|| {
            std::io::Error::other("Missing PATH entry for docker runtime smoke child")
        })?;
    Ok(PathBuf::from(first))
}

#[cfg(unix)]
fn write_failing_id_script(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let script = dir.join("id");
    fs::write(&script, "#!/bin/sh\nexit 1\n")?;
    let mut perms = fs::metadata(&script)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms)?;
    Ok(())
}

#[cfg(unix)]
struct ChildPaths {
    home: PathBuf,
    data: PathBuf,
    state_path: PathBuf,
    plain_dir: PathBuf,
    log: PathBuf,
}

#[cfg(unix)]
struct GitRootChildPaths {
    state_path: PathBuf,
    repo_dir: PathBuf,
    worktree_root: PathBuf,
}

#[cfg(unix)]
struct RuntimeHomeSnapshot {
    staged_home: PathBuf,
    staged_passwd: String,
    staged_group: String,
    container_name: String,
}

#[cfg(unix)]
fn child_paths() -> Result<ChildPaths, Box<dyn std::error::Error>> {
    let home = PathBuf::from(
        std::env::var_os("HOME")
            .ok_or_else(|| std::io::Error::other("Missing HOME for docker runtime smoke child"))?,
    );
    let data = PathBuf::from(std::env::var_os("XDG_DATA_HOME").ok_or_else(|| {
        std::io::Error::other("Missing XDG_DATA_HOME for docker runtime smoke child")
    })?);
    Ok(ChildPaths {
        home,
        data,
        state_path: required_env_path(STATE_PATH_VAR)?,
        plain_dir: required_env_path(PLAIN_DIR_VAR)?,
        log: required_env_path(LOG_PATH_VAR)?,
    })
}

#[cfg(unix)]
fn git_root_child_paths() -> Result<GitRootChildPaths, Box<dyn std::error::Error>> {
    Ok(GitRootChildPaths {
        state_path: required_env_path(STATE_PATH_VAR)?,
        repo_dir: required_env_path(GIT_REPO_DIR_VAR)?,
        worktree_root: required_env_path(WORKTREE_ROOT_VAR)?,
    })
}

#[cfg(unix)]
fn codex_settings(docker_for_new_roots: bool) -> tenex::app::Settings {
    tenex::app::Settings {
        agent_program: tenex::app::AgentProgram::Codex,
        docker_for_new_roots,
        ..tenex::app::Settings::default()
    }
}

#[cfg(unix)]
fn init_git_repo_with_ignored_file(repo_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut init_opts = RepositoryInitOptions::new();
    init_opts.initial_head("master");
    let repo = git2::Repository::init_opts(repo_dir, &init_opts)?;
    repo.set_head("refs/heads/master")?;

    let mut config = repo.config()?;
    config.set_str("user.name", "Test")?;
    config.set_str("user.email", "test@test.com")?;
    config.set_bool("core.autocrlf", false)?;
    config.set_str("core.eol", "lf")?;
    config.set_str("commit.gpgsign", "false")?;

    fs::write(repo_dir.join("README.md"), "# Test Repository\n")?;
    fs::write(repo_dir.join(".gitignore"), "shared-cache.db\n")?;

    let mut index = repo.index()?;
    index.add_path(Path::new("README.md"))?;
    index.add_path(Path::new(".gitignore"))?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let sig = Signature::now("Test", "test@test.com")?;
    repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

    fs::write(repo_dir.join("shared-cache.db"), "host cache\n")?;
    fs::write(repo_dir.join("AGENTS.md"), "# local instructions\n")?;
    std::os::unix::fs::symlink("AGENTS.md", repo_dir.join("CLAUDE.md"))?;
    Ok(())
}

#[cfg(unix)]
fn enable_docker_mode(state_path: &Path) {
    let mut prep_app = tenex::App::new(
        tenex::config::Config::default(),
        tenex::agent::Storage::with_path(state_path.to_path_buf()),
        codex_settings(false),
        false,
    );
    prep_app.data.input.buffer = "/toggle_docker".to_string();
    let _ = prep_app.data.submit_slash_command_palette();
}

#[cfg(unix)]
fn create_docker_git_root_app(
    state_path: &Path,
    repo_dir: &Path,
    worktree_root: &Path,
) -> Result<(tenex::App, tenex::agent::Agent), Box<dyn std::error::Error>> {
    let config = tenex::config::Config {
        worktree_dir: worktree_root.to_path_buf(),
        ..tenex::config::Config::default()
    };
    let mut app = tenex::App::new(
        config,
        tenex::agent::Storage::with_path(state_path.to_path_buf()),
        codex_settings(true),
        false,
    );
    app.set_cwd_project_root(Some(repo_dir.to_path_buf()));
    let handler = tenex::app::Actions::new();

    let next = handler.create_agent(&mut app.data, "docker-root", Some("test prompt"))?;
    app.apply_mode(next);

    let agent = app
        .selected_agent()
        .ok_or_else(|| std::io::Error::other("Expected a Docker git root agent"))?
        .clone();
    assert_eq!(agent.runtime, tenex::agent::AgentRuntime::Docker);
    Ok((app, agent))
}

#[cfg(unix)]
fn create_docker_root_app(
    state_path: &Path,
    plain_dir: &Path,
) -> Result<(tenex::App, tenex::agent::Agent), Box<dyn std::error::Error>> {
    let mut app = tenex::App::new(
        tenex::config::Config::default(),
        tenex::agent::Storage::with_path(state_path.to_path_buf()),
        codex_settings(true),
        false,
    );
    app.set_cwd_project_root(Some(plain_dir.to_path_buf()));
    let handler = tenex::app::Actions::new();

    let next = handler.create_agent(&mut app.data, "docker-root", Some("test prompt"))?;
    app.apply_mode(next);

    let agent = app
        .selected_agent()
        .ok_or_else(|| std::io::Error::other("Expected a Docker root agent"))?
        .clone();
    assert_eq!(agent.runtime, tenex::agent::AgentRuntime::Docker);
    Ok((app, agent))
}

#[cfg(unix)]
fn snapshot_runtime_home(
    data: &Path,
    agent: &tenex::agent::Agent,
) -> Result<RuntimeHomeSnapshot, Box<dyn std::error::Error>> {
    let container_name = format!("tenex-runtime-{}", agent.runtime_scope).to_ascii_lowercase();
    let staged_home = data
        .join("tenex")
        .join("docker-runtime")
        .join(&container_name)
        .join("home");
    Ok(RuntimeHomeSnapshot {
        staged_passwd: fs::read_to_string(staged_home.join(".tenex-passwd"))?,
        staged_group: fs::read_to_string(staged_home.join(".tenex-group"))?,
        staged_home,
        container_name,
    })
}

#[cfg(unix)]
fn spawn_child_agent(
    app: &mut tenex::App,
    agent: &tenex::agent::Agent,
) -> Result<(), Box<dyn std::error::Error>> {
    app.data.spawn.child_count = 1;
    app.data.spawn.spawning_under = Some(agent.id);
    let next = tenex::app::Actions::new().spawn_children(&mut app.data, Some("smoke task"))?;
    app.apply_mode(next);
    assert!(app.data.storage.len() >= 2);
    Ok(())
}

#[cfg(unix)]
fn assert_runtime_home_state(
    snapshot: &RuntimeHomeSnapshot,
    home: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        fs::read_to_string(snapshot.staged_home.join(".ssh").join("config"))?,
        "Host test\n"
    );
    assert_eq!(
        fs::read_to_string(
            snapshot
                .staged_home
                .join(".config")
                .join("ssh")
                .join("config")
        )?,
        "Host xdg-test\n"
    );
    let staged_claude_settings =
        fs::read_to_string(snapshot.staged_home.join(".claude").join("settings.json"))?;
    assert!(staged_claude_settings.contains("\"defaultMode\": \"plan\""));
    assert!(!staged_claude_settings.contains("\"hooks\""));
    assert_eq!(
        fs::read_to_string(snapshot.staged_home.join(".ssh").join("known_hosts"))?,
        "updated-host-key\n"
    );
    assert_eq!(
        fs::read_to_string(snapshot.staged_home.join(".tenex-passwd"))?,
        snapshot.staged_passwd
    );
    assert_eq!(
        fs::read_to_string(snapshot.staged_home.join(".tenex-group"))?,
        snapshot.staged_group
    );
    assert_eq!(
        fs::read_to_string(
            snapshot
                .staged_home
                .join(".claude")
                .join("commands")
                .join("review.md")
        )?,
        "# review\n"
    );
    assert_eq!(
        fs::read_to_string(home.join(".ssh").join("known_hosts"))?,
        "host-key\n"
    );
    Ok(())
}

#[cfg(unix)]
fn assert_docker_runtime_log(
    log: &Path,
    plain_dir: &Path,
    home: &Path,
    container_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    wait_for_log_line(log, "run -d --init")?;
    let log_contents = fs::read_to_string(log)?;
    let external_plan = plain_dir.join("external-plan.md").canonicalize()?;
    assert!(log_contents.contains("LD_PRELOAD=/usr/local/lib/libnss_wrapper.so"));
    assert!(log_contents.contains("NSS_WRAPPER_PASSWD="));
    assert!(log_contents.contains("NSS_WRAPPER_GROUP="));
    assert!(log_contents.contains(&format!("start {container_name}")));
    assert!(log_contents.contains(&format!(
        "{}:{}",
        external_plan.display(),
        external_plan.display()
    )));
    assert!(!log_contents.contains(&format!(
        "{}:{}:ro",
        home.join(".ssh").display(),
        home.join(".ssh").display()
    )));
    assert!(!log_contents.contains(&format!(
        "{}:{}:ro",
        home.join(".config").join("ssh").display(),
        home.join(".config").join("ssh").display()
    )));
    Ok(())
}

#[cfg(unix)]
fn assert_instruction_file_links(
    worktree_path: &Path,
    repo_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let expected_agents = repo_dir.join("AGENTS.md").canonicalize()?;

    let linked_agents = worktree_path.join("AGENTS.md");
    assert!(
        fs::symlink_metadata(&linked_agents)?
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::canonicalize(&linked_agents)?, expected_agents);

    let linked_claude = worktree_path.join("CLAUDE.md");
    assert!(
        fs::symlink_metadata(&linked_claude)?
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&linked_claude)?, PathBuf::from("AGENTS.md"));
    assert_eq!(
        fs::canonicalize(&linked_claude)?,
        repo_dir.join("AGENTS.md").canonicalize()?
    );

    Ok(())
}

#[cfg(unix)]
fn run_child() -> Result<(), Box<dyn std::error::Error>> {
    let paths = child_paths()?;
    enable_docker_mode(&paths.state_path);
    let (mut app, agent) = create_docker_root_app(&paths.state_path, &paths.plain_dir)?;
    let snapshot = snapshot_runtime_home(&paths.data, &agent)?;
    fs::write(
        snapshot.staged_home.join(".ssh").join("known_hosts"),
        "updated-host-key\n",
    )?;

    write_failing_id_script(&first_path_dir()?)?;
    spawn_child_agent(&mut app, &agent)?;
    assert_runtime_home_state(&snapshot, &paths.home)?;
    assert_docker_runtime_log(
        &paths.log,
        &paths.plain_dir,
        &paths.home,
        &snapshot.container_name,
    )?;

    tenex::mux::SessionManager::new().kill(&agent.mux_session)?;
    tenex::cleanup_agent_runtime(&agent)?;
    Ok(())
}

#[cfg(unix)]
fn run_git_root_child() -> Result<(), Box<dyn std::error::Error>> {
    let paths = git_root_child_paths()?;
    let (_app, agent) =
        create_docker_git_root_app(&paths.state_path, &paths.repo_dir, &paths.worktree_root)?;

    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let ignored_path = agent.worktree_path.join("shared-cache.db");
        let maybe_symlink_target = match fs::symlink_metadata(&ignored_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Some(fs::read_link(&ignored_path)?)
            }
            Ok(_) => None,
            Err(err) if err.kind() == ErrorKind::NotFound => None,
            Err(err) => return Err(err.into()),
        };

        if let Some(target) = maybe_symlink_target {
            return Err(format!(
                "Docker git root should not symlink ignored repo files into its worktree: {} -> {}",
                ignored_path.display(),
                target.display()
            )
            .into());
        }

        assert_instruction_file_links(&agent.worktree_path, &paths.repo_dir)?;
        Ok(())
    })();

    tenex::mux::SessionManager::new().kill(&agent.mux_session)?;
    tenex::cleanup_agent_runtime(&agent)?;
    result
}

#[cfg(unix)]
#[test]
fn test_required_env_path_reads_home() -> Result<(), Box<dyn std::error::Error>> {
    let expected = PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| std::io::Error::other("Missing HOME"))?,
    );
    assert_eq!(required_env_path("HOME")?, expected);
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_first_path_dir_reads_current_path() -> Result<(), Box<dyn std::error::Error>> {
    let expected = PathBuf::from(
        std::env::var("PATH")?
            .split(':')
            .find(|segment| !segment.is_empty())
            .ok_or_else(|| std::io::Error::other("Missing PATH entry"))?,
    );
    assert_eq!(first_path_dir()?, expected);
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_codex_settings_sets_expected_fields() {
    let settings = codex_settings(true);
    assert_eq!(settings.agent_program, tenex::app::AgentProgram::Codex);
    assert!(settings.docker_for_new_roots);
}

#[test]
fn test_docker_root_agent_stages_ssh_home_in_integration_path()
-> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os(CHILD_FLAG).is_some() {
        return run_child();
    }

    if skip_if_no_mux() {
        return Ok(());
    }

    let temp = TempDir::new()?;
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let plain_dir = temp.path().join("plain-dir");
    let docker_dir = temp.path().join("docker-bin");
    let state_path = temp.path().join("state.json");
    let mux_socket = temp.path().join("mux.sock");
    let log = temp.path().join("docker.log");

    fs::create_dir_all(home.join(".ssh"))?;
    fs::create_dir_all(home.join(".config").join("ssh"))?;
    fs::create_dir_all(home.join(".claude").join("commands"))?;
    fs::create_dir_all(home.join(".codex").join("sessions"))?;
    fs::create_dir_all(&plain_dir)?;
    fs::create_dir_all(&docker_dir)?;
    fs::write(home.join(".ssh").join("config"), "Host test\n")?;
    fs::write(home.join(".ssh").join("known_hosts"), "host-key\n")?;
    fs::write(
        home.join(".config").join("ssh").join("config"),
        "Host xdg-test\n",
    )?;
    fs::write(
        home.join(".codex").join("config.toml"),
        "model = \"gpt-5.4\"\n",
    )?;
    fs::write(
        home.join(".claude").join("settings.json"),
        r#"{"permissions":{"defaultMode":"plan"},"hooks":{"Stop":[]}}"#,
    )?;
    fs::write(
        home.join(".claude").join("commands").join("review.md"),
        "# review\n",
    )?;
    let external_plan = temp.path().join("external-plan.md");
    fs::write(&external_plan, "# external plan\n")?;
    std::os::unix::fs::symlink(&external_plan, plain_dir.join("external-plan.md"))?;
    write_fake_docker_script(&docker_dir, &log)?;

    let current_exe = std::env::current_exe()?;
    let path = std::env::var("PATH").unwrap_or_default();
    let prefixed_path = format!("{}:{path}", docker_dir.display());
    let output = Command::new(current_exe)
        .arg("--exact")
        .arg("test_docker_root_agent_stages_ssh_home_in_integration_path")
        .arg("--nocapture")
        .env(CHILD_FLAG, "1")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data)
        .env("TENEX_MUX_SOCKET", &mux_socket)
        .env("PATH", prefixed_path)
        .env(STATE_PATH_VAR, &state_path)
        .env(PLAIN_DIR_VAR, &plain_dir)
        .env(LOG_PATH_VAR, &log)
        .output()?;

    assert!(
        output.status.success(),
        "child test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[cfg(not(unix))]
#[test]
fn test_docker_root_agent_stages_ssh_home_in_integration_path() {}

#[cfg(unix)]
#[test]
fn test_docker_git_root_does_not_symlink_ignored_repo_files()
-> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os(GIT_ROOT_CHILD_FLAG).is_some() {
        return run_git_root_child();
    }

    if skip_if_no_mux() {
        return Ok(());
    }

    let temp = TempDir::new()?;
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let repo_dir = temp.path().join("repo");
    let worktree_root = temp.path().join("worktrees");
    let docker_dir = temp.path().join("docker-bin");
    let state_path = temp.path().join("state.json");
    let mux_socket = temp.path().join("mux.sock");
    let log = temp.path().join("docker.log");

    fs::create_dir_all(home.join(".codex").join("sessions"))?;
    fs::create_dir_all(&repo_dir)?;
    fs::create_dir_all(&worktree_root)?;
    fs::create_dir_all(&docker_dir)?;
    fs::write(
        home.join(".codex").join("config.toml"),
        "model = \"gpt-5.4\"\n",
    )?;
    init_git_repo_with_ignored_file(&repo_dir)?;
    write_fake_docker_script(&docker_dir, &log)?;

    let current_exe = std::env::current_exe()?;
    let path = std::env::var("PATH").unwrap_or_default();
    let prefixed_path = format!("{}:{path}", docker_dir.display());
    let output = Command::new(current_exe)
        .arg("--exact")
        .arg("test_docker_git_root_does_not_symlink_ignored_repo_files")
        .arg("--nocapture")
        .env(GIT_ROOT_CHILD_FLAG, "1")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data)
        .env("TENEX_MUX_SOCKET", &mux_socket)
        .env("PATH", prefixed_path)
        .env(STATE_PATH_VAR, &state_path)
        .env(GIT_REPO_DIR_VAR, &repo_dir)
        .env(WORKTREE_ROOT_VAR, &worktree_root)
        .output()?;

    assert!(
        output.status.success(),
        "child test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[cfg(not(unix))]
#[test]
fn test_docker_git_root_does_not_symlink_ignored_repo_files() {}
