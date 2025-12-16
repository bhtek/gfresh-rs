use atty::Stream;
use owo_colors::OwoColorize;
use std::env;

pub(crate) fn should_use_color() -> bool {
    let is_tty = atty::is(Stream::Stderr) || atty::is(Stream::Stdout);
    is_tty && env::var_os("NO_COLOR").is_none()
}

pub(crate) struct Palette {
    pub(crate) enabled: bool,
}

impl Palette {
    pub(crate) fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    pub(crate) fn good<T: std::fmt::Display>(&self, msg: T) -> String {
        if self.enabled {
            format!("{}", msg.green())
        } else {
            msg.to_string()
        }
    }

    pub(crate) fn warn<T: std::fmt::Display>(&self, msg: T) -> String {
        if self.enabled {
            format!("{}", msg.yellow())
        } else {
            msg.to_string()
        }
    }

    pub(crate) fn info<T: std::fmt::Display>(&self, msg: T) -> String {
        if self.enabled {
            format!("{}", msg.blue())
        } else {
            msg.to_string()
        }
    }
}
