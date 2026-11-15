// Girovo CLI: your accounts and bookings from Girovo in the terminal.
//
// An example app for the Girovo API (https://developer.girovo.de). It talks
// to the Girovo that runs on this computer, through its pipe (Windows) or
// socket (Linux); Girovo asks you once which permissions it gets, like for
// any other app, and you can take them back in Einstellungen → Girovo API.
// Transfers are only suggested: you check and send them in Girovo.

mod commands;
mod girovo;
mod show;

use std::process::ExitCode;

use show::tr;

/// Exit codes: what a script can tell apart.
pub mod exit {
  pub const OK: u8 = 0;
  /// Girovo answered with an error, or a transfer did not go through.
  pub const FAILED: u8 = 1;
  /// The command line was wrong.
  pub const USAGE: u8 = 2;
  /// Girovo is not running, or Girovo API is off.
  pub const NOT_RUNNING: u8 = 3;
  /// You said no in Girovo (or did not answer).
  pub const NOT_ALLOWED: u8 = 4;
}

/// What the command line asked for.
pub struct Args {
  pub json: bool,
  pub address: Option<String>,
  pub command: String,
  /// The command's own words, without the options below.
  pub words: Vec<String>,
  /// `--name value` and `--flag` (value None), by name without dashes.
  pub options: Vec<(String, Option<String>)>,
}

impl Args {
  pub fn option(&self, names: &[&str]) -> Option<&str> {
    self
      .options
      .iter()
      .rev()
      .find(|(k, _)| names.contains(&k.as_str()))
      .and_then(|(_, v)| v.as_deref())
  }
  pub fn flag(&self, names: &[&str]) -> bool {
    self.options.iter().any(|(k, _)| names.contains(&k.as_str()))
  }
}

/// Options that take a value, with their short forms.
const VALUE_OPTIONS: &[(&str, &str)] = &[
  ("account", "a"),
  ("from", ""),
  ("to", ""),
  ("query", "q"),
  ("category", "c"),
  ("limit", "n"),
  ("name", ""),
  ("purpose", "p"),
  ("on", ""),
  ("kinds", "k"),
  ("permissions", ""),
  ("address", ""),
];
const FLAG_OPTIONS: &[(&str, &str)] = &[("all", ""), ("ids", ""), ("json", ""), ("no-color", ""), ("help", "h"), ("version", "V")];

fn parse(raw: Vec<String>) -> Result<Args, String> {
  let mut words = Vec::new();
  let mut options = Vec::new();
  let mut it = raw.into_iter();
  let mut rest_are_words = false;
  while let Some(arg) = it.next() {
    if rest_are_words || arg == "-" || !arg.starts_with('-') || arg.len() == 1 || is_number(&arg) {
      words.push(arg);
      continue;
    }
    if arg == "--" {
      rest_are_words = true;
      continue;
    }
    let (name, inline) = match arg.strip_prefix("--") {
      Some(long) => match long.split_once('=') {
        Some((k, v)) => (k.to_string(), Some(v.to_string())),
        None => (long.to_string(), None),
      },
      None => {
        let short = &arg[1..];
        let found = VALUE_OPTIONS.iter().chain(FLAG_OPTIONS).find(|(_, s)| *s == short);
        match found {
          Some((long, _)) => (long.to_string(), None),
          None => return Err(format!("{} {arg}", tr("Unbekannte Option:", "Unknown option:"))),
        }
      }
    };
    if VALUE_OPTIONS.iter().any(|(l, _)| *l == name) {
      let value = match inline {
        Some(v) => v,
        None => it.next().ok_or_else(|| format!("--{name} {}", tr("braucht einen Wert.", "needs a value.")))?,
      };
      options.push((name, Some(value)));
    } else if FLAG_OPTIONS.iter().any(|(l, _)| *l == name) {
      options.push((name, None));
    } else {
      return Err(format!("{} --{name}", tr("Unbekannte Option:", "Unknown option:")));
    }
  }
  let command = if words.is_empty() { String::new() } else { words.remove(0) };
  let mut args = Args {
    json: false,
    address: None,
    command,
    words,
    options,
  };
  args.json = args.flag(&["json"]);
  args.address = args.option(&["address"]).map(String::from);
  Ok(args)
}

/// "-12,30" is an amount, not an option.
fn is_number(s: &str) -> bool {
  s.strip_prefix('-').is_some_and(|r| r.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

fn main() -> ExitCode {
  let raw: Vec<String> = std::env::args().skip(1).collect();
  show::init(raw.iter().any(|a| a == "--no-color"));
  let args = match parse(raw) {
    Ok(a) => a,
    Err(e) => {
      eprintln!("{e}");
      eprintln!("{}", tr("Hilfe: girovo-cli --help", "Help: girovo-cli --help"));
      return ExitCode::from(exit::USAGE);
    }
  };
  if args.flag(&["version"]) {
    println!("Girovo CLI {}", env!("CARGO_PKG_VERSION"));
    return ExitCode::from(exit::OK);
  }
  if args.flag(&["help"]) || args.command.is_empty() || args.command == "help" || args.command == "hilfe" {
    commands::help();
    return ExitCode::from(exit::OK);
  }
  ExitCode::from(commands::run(&args))
}
