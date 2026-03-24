//! Disposable integration repro for Docker git-root worktree isolation.

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::ErrorKind;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use git2::{RepositoryInitOptions, Signature};
#[cfg(unix)]
use tempfile::TempDir;

#[cfg(unix)]
const CHILD_FLAG: &str = "TENEX_REPRO_DOCKER_GIT_ROOT_CHILD";
#[cfg(unix)]
const STATE_PATH_VAR: &str = "TENEX_REPRO_DOCKER_GIT_ROOT_STATE_PATH";
#[cfg(unix)]
const GIT_REPO_DIR_VAR: &str = "TENEX_REPRO_DOCKER_GIT_ROOT_REPO_DIR";
#[cfg(unix)]
const WORKTREE_ROOT_VAR: &str = "TENEX_REPRO_DOCKER_GIT_ROOT_WORKTREE_ROOT";

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
struct ChildPaths {
    state_path: PathBuf,
    repo_dir: PathBuf,
    worktree_root: PathBuf,
}

#[cfg(unix)]
fn required_env_path(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(std::env::var_os(name).ok_or_else(|| {
        std::io::Error::other(format!("Missing required env var {name}"))
    })?))
}

#[cfg(unix)]
fn child_paths() -> Result<ChildPaths, Box<dyn std::error::Error>> {
    Ok(ChildPaths {
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
fn init_git_repo_with_fixture(repo_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
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
fn write_fake_docker_script(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let script = dir.join("docker");
    let state = dir.join("docker-state");
    fs::write(
        &script,
        format!(
            "#!/bin/sh
set -eu
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
            state = state.display(),
            layout_hash = current_container_layout_hash(),
        ),
    )?;
    let mut perms = fs::metadata(&script)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms)?;
    Ok(())
}

#[cfg(unix)]
fn create_docker_git_root_app(
    state_path: &Path,
    repo_dir: &Path,
    worktree_root: &Path,
) -> Result<tenex::agent::Agent, Box<dyn std::error::Error>> {
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

    let next = tenex::app::Actions::new().create_agent(
        &mut app.data,
        "docker-root",
        Some("repro prompt"),
    )?;
    app.apply_mode(next);

    let agent = app
        .selected_agent()
        .ok_or_else(|| std::io::Error::other("Expected a Docker git root agent"))?
        .clone();
    assert_eq!(agent.runtime, tenex::agent::AgentRuntime::Docker);
    Ok(agent)
}

#[cfg(unix)]
fn assert_instruction_file_links(
    worktree_path: &Path,
    repo_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let expected_agents = repo_dir.join("AGENTS.md").canonicalize()?;

    let linked_agents = worktree_path.join("AGENTS.md");
    assert!(fs::symlink_metadata(&linked_agents)?.file_type().is_symlink());
    assert_eq!(fs::canonicalize(&linked_agents)?, expected_agents);

    let linked_claude = worktree_path.join("CLAUDE.md");
    assert!(fs::symlink_metadata(&linked_claude)?.file_type().is_symlink());
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
    let agent = create_docker_git_root_app(&paths.state_path, &paths.repo_dir, &paths.worktree_root)?;

    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let ignored_path = agent.worktree_path.join("shared-cache.db");
        let maybe_symlink_target = match fs::symlink_metadata(&ignored_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => Some(fs::read_link(&ignored_path)?),
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
fn test_repro_docker_git_root_instruction_links_and_isolation()
-> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os(CHILD_FLAG).is_some() {
        return run_child();
    }

    let temp = TempDir::new()?;
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let repo_dir = temp.path().join("repo");
    let worktree_root = temp.path().join("worktrees");
    let docker_dir = temp.path().join("docker-bin");
    let state_path = temp.path().join("state.json");
    let mux_socket = temp.path().join("mux.sock");

    fs::create_dir_all(home.join(".codex").join("sessions"))?;
    fs::create_dir_all(&data)?;
    fs::create_dir_all(&repo_dir)?;
    fs::create_dir_all(&worktree_root)?;
    fs::create_dir_all(&docker_dir)?;
    fs::write(
        home.join(".codex").join("config.toml"),
        "model = \"gpt-5.4\"\n",
    )?;
    init_git_repo_with_fixture(&repo_dir)?;
    write_fake_docker_script(&docker_dir)?;

    let current_exe = std::env::current_exe()?;
    let path = std::env::var("PATH").unwrap_or_default();
    let prefixed_path = format!("{}:{path}", docker_dir.display());
    let output = Command::new(current_exe)
        .arg("--exact")
        .arg("test_repro_docker_git_root_instruction_links_and_isolation")
        .arg("--nocapture")
        .env(CHILD_FLAG, "1")
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
fn test_repro_docker_git_root_instruction_links_and_isolation() {}
