use anyhow::{anyhow, bail, Context, Result};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_debug(enabled: bool) {
    DEBUG.store(enabled, Ordering::Relaxed);
}

fn debug_log(message: impl AsRef<str>) {
    if DEBUG.load(Ordering::Relaxed) {
        eprintln!("[debug] {}", message.as_ref());
    }
}

pub(crate) fn run_git(args: &[&str]) -> Result<String> {
    debug_log(format!("git {}", args.join(" ")));
    let output = Command::new("git")
        .args(args)
        .env("GIT_PAGER", "")
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout.trim_end().to_string())
}

pub(crate) fn ensure_git_repo() -> Result<()> {
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .env("GIT_PAGER", "")
        .output()
        .context("failed to check whether this is a git repository")?;

    if !output.status.success() {
        bail!("not inside a git work tree (run gfresh from within a repository)");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim() != "true" {
        bail!("not inside a git work tree (run gfresh from within a repository)");
    }

    Ok(())
}

pub(crate) fn ensure_remote(remote: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["remote", "get-url", remote])
        .env("GIT_PAGER", "")
        .output()
        .with_context(|| format!("failed to check for remote '{remote}'"))?;

    if !output.status.success() {
        bail!("no remote named '{remote}' is configured");
    }

    Ok(())
}

pub(crate) fn git_status_dirty() -> Result<(bool, Vec<String>)> {
    let output = run_git(&["status", "--porcelain"])?;
    let lines: Vec<String> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect();
    Ok((!lines.is_empty(), lines))
}

pub(crate) fn detect_main_branch() -> Result<String> {
    if let Some(branch) = origin_default_branch()? {
        return Ok(branch);
    }

    for candidate in ["main", "develop", "master"] {
        if local_branch_exists(candidate)? || origin_branch_exists(candidate)? {
            return Ok(candidate.to_string());
        }
    }
    bail!("Could not find a main branch (tried main/develop/master)");
}

fn origin_default_branch() -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"])
        .env("GIT_PAGER", "")
        .output()
        .context("failed to read origin default branch")?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_origin_head_ref(&stdout))
}

fn git_ref_exists(name: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", name])
        .env("GIT_PAGER", "")
        .output()
        .with_context(|| format!("failed to probe ref {name}"))?;
    Ok(output.status.success())
}

fn local_branch_exists(name: &str) -> Result<bool> {
    git_ref_exists(&format!("refs/heads/{name}"))
}

pub(crate) fn origin_branch_exists(name: &str) -> Result<bool> {
    git_ref_exists(&format!("refs/remotes/origin/{name}"))
}

pub(crate) fn checkout_branch(name: &str) -> Result<()> {
    if local_branch_exists(name)? {
        run_git(&["checkout", name])?;
        return Ok(());
    }

    if origin_branch_exists(name)? {
        run_git(&["checkout", "-t", &format!("origin/{name}")])?;
        return Ok(());
    }

    bail!("branch '{name}' not found locally or on origin");
}

pub(crate) fn current_branch() -> Result<String> {
    run_git(&["symbolic-ref", "--short", "HEAD"])
}

pub(crate) fn ahead_behind(local_ref: &str, remote_ref: &str) -> Result<(u32, u32)> {
    let range = format!("{local_ref}...{remote_ref}");
    let output = run_git(&["rev-list", "--left-right", "--count", &range])?;
    let mut parts = output.split_whitespace();
    let ahead = parts
        .next()
        .ok_or_else(|| anyhow!("unexpected rev-list output: {output}"))?
        .parse::<u32>()
        .context("failed parsing ahead count")?;
    let behind = parts
        .next()
        .ok_or_else(|| anyhow!("unexpected rev-list output: {output}"))?
        .parse::<u32>()
        .context("failed parsing behind count")?;
    Ok((ahead, behind))
}

pub(crate) fn delete_stale_branches(main_branch: &str) -> Result<Vec<String>> {
    let output = run_git(&["branch", "-vv", "--no-color"])?;
    let mut deleted = Vec::new();

    for line in output.lines() {
        let Some(info) = parse_branch_vv_line(line) else {
            continue;
        };

        if should_delete_stale_branch(&info, main_branch) {
            debug_log(format!("Deleting stale branch {}", info.name));
            run_git(&["branch", "-D", &info.name])?;
            deleted.push(info.name);
        }
    }

    Ok(deleted)
}

pub(crate) fn recent_commits(count: usize) -> Result<String> {
    run_git(&[
        "log",
        "--graph",
        "--decorate",
        "--oneline",
        &format!("-n{count}"),
    ])
}

fn parse_origin_head_ref(stdout: &str) -> Option<String> {
    let full_ref = stdout.trim();
    const PREFIX: &str = "refs/remotes/origin/";
    full_ref
        .strip_prefix(PREFIX)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchVvLine {
    name: String,
    is_current: bool,
    is_gone: bool,
}

fn parse_branch_vv_line(line: &str) -> Option<BranchVvLine> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    let mut parts = trimmed.split_whitespace();
    let first = parts.next()?;

    let (is_current, name) = if first == "*" {
        (true, parts.next()?)
    } else {
        (false, first)
    };

    Some(BranchVvLine {
        name: name.to_string(),
        is_current,
        is_gone: tracking_is_gone(trimmed),
    })
}

fn tracking_is_gone(line: &str) -> bool {
    line.contains(": gone]") || line.contains("[gone]")
}

fn should_delete_stale_branch(info: &BranchVvLine, main_branch: &str) -> bool {
    info.is_gone && !info.is_current && info.name != main_branch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_origin_head_main() {
        assert_eq!(
            parse_origin_head_ref("refs/remotes/origin/main\n"),
            Some("main".to_string())
        );
    }

    #[test]
    fn parse_origin_head_invalid() {
        assert_eq!(parse_origin_head_ref(""), None);
        assert_eq!(parse_origin_head_ref("refs/heads/main\n"), None);
        assert_eq!(parse_origin_head_ref("refs/remotes/origin/\n"), None);
    }

    #[test]
    fn parse_branch_vv_current() {
        let info = parse_branch_vv_line("* main 123abcd [origin/main] msg").unwrap();
        assert_eq!(
            info,
            BranchVvLine {
                name: "main".to_string(),
                is_current: true,
                is_gone: false,
            }
        );
    }

    #[test]
    fn parse_branch_vv_gone() {
        let info = parse_branch_vv_line("  feature 123abcd [origin/feature: gone] msg").unwrap();
        assert_eq!(info.name, "feature");
        assert!(!info.is_current);
        assert!(info.is_gone);
    }

    #[test]
    fn gone_detection_compat() {
        assert!(tracking_is_gone("[gone]"));
        assert!(tracking_is_gone("[origin/x: gone]"));
        assert!(!tracking_is_gone("[origin/x]"));
    }

    #[test]
    fn should_delete_respects_main_and_current() {
        let base = BranchVvLine {
            name: "main".to_string(),
            is_current: false,
            is_gone: true,
        };
        assert!(!should_delete_stale_branch(&base, "main"));

        let current = BranchVvLine {
            name: "feat".to_string(),
            is_current: true,
            is_gone: true,
        };
        assert!(!should_delete_stale_branch(&current, "main"));

        let stale = BranchVvLine {
            name: "feat".to_string(),
            is_current: false,
            is_gone: true,
        };
        assert!(should_delete_stale_branch(&stale, "main"));
    }
}
