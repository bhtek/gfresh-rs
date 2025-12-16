//! gfresh: refresh your repo to the main branch.
//!
//! Usage:
//! ```
//! gfresh [-f|--force] [-d|--debug]
//! ```
//! - `--force` hard-resets if the working tree is dirty.
//! - `--debug` prints the git commands being run.
//! Run from inside a git repository.

use anyhow::{anyhow, bail, Context, Result};
use atty::Stream;
use owo_colors::OwoColorize;
use std::env;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG: AtomicBool = AtomicBool::new(false);

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let config = parse_args()?;
    DEBUG.store(config.debug, Ordering::Relaxed);
    let palette = Palette::new(should_use_color());

    let (dirty, changes) = git_status_dirty()?;
    if dirty && !config.force {
        eprintln!(
            "{}",
            palette.warn("Working tree is dirty. Use --force to reset.")
        );
        if !changes.is_empty() {
            eprintln!("Changes:");
            for line in changes {
                eprintln!("  {line}");
            }
        }
        bail!("Aborting to avoid losing local changes");
    }

    if dirty && config.force {
        eprintln!(
            "{}",
            palette.warn("Working tree dirty, resetting with --hard")
        );
        run_git(&["reset", "--hard", "HEAD"])?;
    }

    let main_branch = detect_main_branch()?;
    eprintln!(
        "{} {}",
        palette.info("Main branch detected:"),
        main_branch.bold()
    );

    let current_branch = current_branch()?;
    if current_branch != main_branch {
        eprintln!(
            "{} {} -> {}",
            palette.info("Switching branches:"),
            current_branch,
            main_branch
        );
        run_git(&["checkout", &main_branch])?;
    }

    let remote_ref = format!("origin/{main_branch}");
    let before_counts = ahead_behind("HEAD", &remote_ref).ok();

    eprintln!("{}", palette.info("Fetching from origin (with prune)..."));
    run_git(&["fetch", "--prune", "origin"])?;

    let (ahead, behind) = ahead_behind("HEAD", &remote_ref)?;
    let fetched_commits = behind;
    let to_rebase = behind;

    eprintln!(
        "{} {} ahead / {} behind (before)",
        palette.info("Ahead/behind:"),
        format_count_opt(before_counts.map(|c| c.0), palette.good_enabled()),
        format_count_opt(before_counts.map(|c| c.1), palette.good_enabled())
    );
    eprintln!(
        "{} {} ahead / {} behind (after fetch)",
        palette.info("Ahead/behind:"),
        palette.good(ahead),
        palette.good(behind)
    );
    eprintln!(
        "{} {} commits fetched; {} to rebase",
        palette.info("Sync summary:"),
        palette.good(fetched_commits),
        palette.good(to_rebase)
    );

    if to_rebase == 0 {
        eprintln!("{}", palette.info("Already up to date, skipping rebase"));
    } else {
        eprintln!("{}", palette.info("Rebasing onto fetched origin/main..."));
        run_git(&["rebase", &remote_ref])?;
    }
    let after_rebase_counts = ahead_behind("HEAD", &remote_ref)?;
    eprintln!(
        "{} {} ahead / {} behind (post-rebase)",
        palette.info("Ahead/behind:"),
        palette.good(after_rebase_counts.0),
        palette.good(after_rebase_counts.1)
    );

    let cleaned = delete_stale_branches(&main_branch)?;
    if cleaned.is_empty() {
        eprintln!("{}", palette.info("No stale branches to delete"));
    } else {
        eprintln!(
            "{} {}",
            palette.info("Deleted stale branches:"),
            cleaned.join(", ")
        );
    }

    eprintln!("{}", palette.info("Recent commits:"));
    let recent = recent_commits(3)?;
    println!("{recent}");

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct Config {
    force: bool,
    debug: bool,
}

fn parse_args() -> Result<Config> {
    let mut force = false;
    let mut debug = false;

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "-f" | "--force" => force = true,
            "-d" | "--debug" => debug = true,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => bail!("Unknown argument: {other}"),
        }
    }

    Ok(Config { force, debug })
}

fn print_usage() {
    println!("gfresh - refresh a git repository");
    println!("Usage: gfresh [-f|--force] [-d|--debug]");
}

fn should_use_color() -> bool {
    atty::is(Stream::Stdout) && env::var_os("NO_COLOR").is_none()
}

struct Palette {
    enabled: bool,
}

impl Palette {
    fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    fn good_enabled(&self) -> bool {
        self.enabled
    }

    fn good<T: std::fmt::Display>(&self, msg: T) -> String {
        if self.enabled {
            format!("{}", msg.green())
        } else {
            msg.to_string()
        }
    }

    fn warn<T: std::fmt::Display>(&self, msg: T) -> String {
        if self.enabled {
            format!("{}", msg.yellow())
        } else {
            msg.to_string()
        }
    }

    fn info<T: std::fmt::Display>(&self, msg: T) -> String {
        if self.enabled {
            format!("{}", msg.blue())
        } else {
            msg.to_string()
        }
    }
}

fn debug_log(message: impl AsRef<str>) {
    if DEBUG.load(Ordering::Relaxed) {
        eprintln!("[debug] {}", message.as_ref());
    }
}

fn run_git(args: &[&str]) -> Result<String> {
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

fn git_status_dirty() -> Result<(bool, Vec<String>)> {
    let output = run_git(&["status", "--porcelain"])?;
    let lines: Vec<String> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect();
    Ok((!lines.is_empty(), lines))
}

fn detect_main_branch() -> Result<String> {
    for candidate in ["main", "develop", "master"] {
        if git_ref_exists(candidate)? {
            return Ok(candidate.to_string());
        }
    }
    bail!("Could not find a main branch (tried main/develop/master)");
}

fn git_ref_exists(name: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", name])
        .env("GIT_PAGER", "")
        .output()
        .with_context(|| format!("failed to probe ref {name}"))?;
    Ok(output.status.success())
}

fn current_branch() -> Result<String> {
    run_git(&["symbolic-ref", "--short", "HEAD"])
}

fn ahead_behind(local_ref: &str, remote_ref: &str) -> Result<(u32, u32)> {
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

fn delete_stale_branches(main_branch: &str) -> Result<Vec<String>> {
    let output = run_git(&["branch", "-vv", "--no-color"])?;
    let mut deleted = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim_start();
        let mut parts = trimmed.split_whitespace();
        let first = match parts.next() {
            Some(v) => v,
            None => continue,
        };

        let (is_current, name) = if first == "*" {
            (true, parts.next())
        } else {
            (false, Some(first))
        };

        let branch_name = match name {
            Some(n) => n,
            None => continue,
        };

        if trimmed.contains("[gone]") && !is_current && branch_name != main_branch {
            debug_log(format!("Deleting stale branch {branch_name}"));
            run_git(&["branch", "-D", branch_name])?;
            deleted.push(branch_name.to_string());
        }
    }

    Ok(deleted)
}

fn recent_commits(count: usize) -> Result<String> {
    run_git(&[
        "log",
        "--graph",
        "--decorate",
        "--oneline",
        &format!("-n{count}"),
    ])
}

fn format_count_opt(count: Option<u32>, colored: bool) -> String {
    match count {
        Some(value) => {
            if colored {
                format!("{}", value.green())
            } else {
                value.to_string()
            }
        }
        None => "n/a".to_string(),
    }
}
