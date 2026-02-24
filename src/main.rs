//! gfresh: refresh your repo to the main branch.
//!
//! Usage:
//! ```
//! gfresh [-f|--force] [-d|--debug]
//! ```
//! - `--force` hard-resets if the working tree is dirty.
//! - `--debug` prints the git commands being run.
//!
//! Run from inside a git repository.

mod cli;
mod git;
mod ui;

use anyhow::{bail, Result};
use owo_colors::OwoColorize;
use std::process::ExitCode;

/// Ensures we're on the specified branch, switching to it if necessary.
fn ensure_on_branch(main_branch: &str, palette: &ui::Palette) -> Result<()> {
    let current_branch = git::current_branch().unwrap_or_else(|_| "HEAD".to_string());

    if current_branch != "HEAD" && current_branch != main_branch {
        eprintln!(
            "{} {} -> {}",
            palette.info("Switching branches:"),
            current_branch,
            main_branch
        );
        git::checkout_branch(main_branch)?;
    } else if current_branch == "HEAD" {
        eprintln!(
            "{} {}",
            palette.warn("Detached HEAD; checking out"),
            main_branch.bold()
        );
        git::checkout_branch(main_branch)?;
    }

    Ok(())
}

/// Prints an ahead/behind summary line with a label.
fn print_ahead_behind(label: &str, ahead: Option<u32>, behind: Option<u32>, palette: &ui::Palette) {
    eprintln!(
        "{} {} ahead / {} behind ({})",
        palette.info("Ahead/behind:"),
        palette.good_opt(ahead),
        palette.good_opt(behind),
        label
    );
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let config = cli::parse_args()?;
    git::set_debug(config.debug);
    let palette = ui::Palette::new(ui::should_use_color());

    git::ensure_git_repo()?;
    git::ensure_remote("origin")?;

    let (dirty, changes) = git::git_status_dirty()?;
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
        git::run_git(&["reset", "--hard", "HEAD"])?;
    }

    let main_branch = git::detect_main_branch()?;
    eprintln!(
        "{} {}",
        palette.info("Main branch detected:"),
        main_branch.bold()
    );

    ensure_on_branch(&main_branch, &palette)?;

    let remote_ref = format!("origin/{main_branch}");
    let before_counts = git::ahead_behind("HEAD", &remote_ref).ok();

    eprintln!("{}", palette.info("Fetching from origin (with prune)..."));
    git::run_git(&["fetch", "--prune", "origin"])?;

    if !git::origin_branch_exists(&main_branch)? {
        bail!("remote branch 'origin/{main_branch}' not found after fetch");
    }

    let (ahead, behind) = git::ahead_behind("HEAD", &remote_ref)?;

    print_ahead_behind("before", before_counts.map(|c| c.0), before_counts.map(|c| c.1), &palette);
    print_ahead_behind("after fetch", Some(ahead), Some(behind), &palette);
    eprintln!(
        "{} {} commits to integrate (rebase)",
        palette.info("Sync summary:"),
        palette.good(behind)
    );

    if behind == 0 {
        eprintln!("{}", palette.info("Already up to date, skipping rebase"));
    } else {
        eprintln!("{} {}", palette.info("Rebasing onto"), remote_ref.bold());
        git::run_git(&["rebase", &remote_ref])?;
    }
    let after_rebase_counts = git::ahead_behind("HEAD", &remote_ref)?;
    print_ahead_behind("post-rebase", Some(after_rebase_counts.0), Some(after_rebase_counts.1), &palette);

    let cleaned = git::delete_stale_branches(&main_branch)?;
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
    let recent = git::recent_commits(3)?;
    println!("{recent}");

    Ok(())
}
