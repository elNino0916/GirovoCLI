// How Girovo CLI talks and prints: the language (German, else English, as
// Girovo itself), amounts and days as Girovo writes them, colour, and plain
// tables that fit the terminal.

use std::io::IsTerminal;
use std::sync::OnceLock;

use serde_json::Value;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, PartialEq)]
pub enum Lang {
  De,
  En,
}

static LANG: OnceLock<Lang> = OnceLock::new();
static COLOR: OnceLock<bool> = OnceLock::new();

pub fn init(no_color: bool) {
  let _ = LANG.set(detect_lang());
  let wanted = !no_color && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) && std::io::stdout().is_terminal();
  let _ = COLOR.set(wanted && os::enable_ansi());
}

pub fn lang() -> Lang {
  *LANG.get().unwrap_or(&Lang::En)
}

/// The German text, or the English one.
pub fn tr(de: &'static str, en: &'static str) -> &'static str {
  if lang() == Lang::De { de } else { en }
}

fn detect_lang() -> Lang {
  let pick = |s: &str| if s.to_ascii_lowercase().starts_with("de") { Lang::De } else { Lang::En };
  if let Some(v) = std::env::var("GIROVO_LANG").ok().filter(|v| !v.is_empty()) {
    return pick(&v);
  }
  for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
    if let Some(v) = std::env::var(key).ok().filter(|v| !v.is_empty() && v != "C" && v != "POSIX") {
      return pick(&v);
    }
  }
  os::system_lang().unwrap_or(Lang::En)
}

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

fn color() -> bool {
  *COLOR.get().unwrap_or(&false)
}

fn paint(code: &str, s: &str) -> String {
  if color() && !s.is_empty() {
    format!("\x1b[{code}m{s}\x1b[0m")
  } else {
    s.to_string()
  }
}

/// Headings: bold, in Girovo Blue lifted for dark terminals.
pub fn heading(s: &str) -> String {
  paint("1;38;2;94;162;230", s)
}
pub fn bold(s: &str) -> String {
  paint("1", s)
}
pub fn dim(s: &str) -> String {
  paint("2", s)
}
pub fn green(s: &str) -> String {
  paint("32", s)
}
pub fn red(s: &str) -> String {
  paint("31", s)
}
pub fn blue(s: &str) -> String {
  paint("38;2;94;162;230", s)
}

// ---------------------------------------------------------------------------
// Amounts and days
// ---------------------------------------------------------------------------

/// Cents as Girovo writes them: "1.234,56 €" / "€1,234.56".
pub fn cents(c: i64, currency: &str) -> String {
  let sign = if c < 0 { "-" } else { "" };
  let abs = c.unsigned_abs();
  let (whole, part) = (abs / 100, abs % 100);
  let (group, decimal) = if lang() == Lang::De { ('.', ',') } else { (',', '.') };
  let digits = whole.to_string();
  let mut grouped = String::new();
  for (i, ch) in digits.chars().enumerate() {
    if i > 0 && (digits.len() - i) % 3 == 0 {
      grouped.push(group);
    }
    grouped.push(ch);
  }
  let number = format!("{grouped}{decimal}{part:02}");
  match (lang(), currency) {
    (Lang::De, "EUR") => format!("{sign}{number} €"),
    (Lang::En, "EUR") => format!("{sign}€{number}"),
    _ => format!("{sign}{number} {currency}"),
  }
}

/// `{ cents, currency }` as text; "" for null.
pub fn money(v: &Value) -> String {
  match v.get("cents").and_then(Value::as_i64) {
    Some(c) => cents(c, v.get("currency").and_then(Value::as_str).unwrap_or("EUR")),
    None => String::new(),
  }
}

/// An amount coloured by its direction: money coming in is green.
pub fn money_signed(v: &Value) -> String {
  let text = money(v);
  if v.get("cents").and_then(Value::as_i64).unwrap_or(0) > 0 {
    green(&text)
  } else {
    text
  }
}

const MONTHS_DE: [&str; 12] = [
  "Januar",
  "Februar",
  "März",
  "April",
  "Mai",
  "Juni",
  "Juli",
  "August",
  "September",
  "Oktober",
  "November",
  "Dezember",
];
const MONTHS_EN: [&str; 12] = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

/// "yyyy-mm-dd" as "15.11.2026" / "15 Nov 2026".
pub fn day(d: &str) -> String {
  let parts: Vec<&str> = d.split('-').collect();
  if parts.len() != 3 {
    return d.to_string();
  }
  let m: usize = parts[1].parse().unwrap_or(0);
  match lang() {
    Lang::De => format!("{}.{}.{}", parts[2], parts[1], parts[0]),
    Lang::En if (1..=12).contains(&m) => format!("{} {} {}", parts[2], &MONTHS_EN[m - 1][..3], parts[0]),
    Lang::En => d.to_string(),
  }
}

/// "yyyy-mm" as "November 2026".
pub fn month(m: &str) -> String {
  let (y, mo) = m.split_once('-').unwrap_or((m, ""));
  match mo.parse::<usize>() {
    Ok(n @ 1..=12) => format!("{} {y}", if lang() == Lang::De { MONTHS_DE[n - 1] } else { MONTHS_EN[n - 1] }),
    _ => m.to_string(),
  }
}

/// An IBAN in groups of four.
pub fn iban(s: &str) -> String {
  let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
  compact
    .as_bytes()
    .chunks(4)
    .map(|c| String::from_utf8_lossy(c).into_owned())
    .collect::<Vec<_>>()
    .join(" ")
}

pub fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
  v.get(key).and_then(Value::as_str).unwrap_or("")
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
  Left,
  Right,
}

/// A cell: what is shown and its plain text (for widths and cutting).
pub struct Cell {
  plain: String,
  style: fn(&str) -> String,
}

fn same(s: &str) -> String {
  s.to_string()
}

impl Cell {
  pub fn new(s: impl Into<String>) -> Cell {
    Cell { plain: s.into(), style: same }
  }
  pub fn styled(s: impl Into<String>, style: fn(&str) -> String) -> Cell {
    Cell { plain: s.into(), style }
  }
}

pub struct Table {
  align: Vec<Align>,
  /// The column that gives way when the terminal is too narrow.
  flex: Option<usize>,
  head: Option<Vec<Cell>>,
  rows: Vec<Vec<Cell>>,
}

impl Table {
  pub fn new(align: &[Align], flex: Option<usize>) -> Table {
    Table {
      align: align.to_vec(),
      flex,
      head: None,
      rows: Vec::new(),
    }
  }

  /// The headings, printed dim above the rows.
  pub fn head(&mut self, cells: Vec<Cell>) {
    self.head = Some(cells);
  }

  pub fn row(&mut self, cells: Vec<Cell>) {
    self.rows.push(cells);
  }

  pub fn is_empty(&self) -> bool {
    self.rows.is_empty()
  }

  pub fn print(&self) {
    let n = self.align.len();
    // A column with nothing in any row is left out, its heading too.
    let shown: Vec<usize> = (0..n)
      .filter(|&i| self.rows.iter().any(|r| r.get(i).is_some_and(|c| !c.plain.is_empty())))
      .collect();
    let mut widths = vec![0usize; n];
    for row in self.head.iter().chain(&self.rows) {
      for (i, cell) in row.iter().enumerate().take(n) {
        widths[i] = widths[i].max(cell.plain.width());
      }
    }
    // Two spaces in front, two between columns.
    if let (Some(flex), Some(term)) = (self.flex, os::terminal_width()) {
      let total: usize = 2 + shown.iter().map(|&i| widths[i]).sum::<usize>() + 2 * shown.len().saturating_sub(1);
      if total > term && shown.contains(&flex) {
        widths[flex] = widths[flex].saturating_sub(total - term).max(8);
      }
    }
    for row in self.head.iter().chain(&self.rows) {
      let cells: Vec<(usize, &Cell)> = shown.iter().filter_map(|&i| row.get(i).map(|c| (i, c))).collect();
      let mut line = String::from("  ");
      for (k, (i, cell)) in cells.iter().enumerate() {
        let text = cut(&cell.plain, widths[*i]);
        let pad = " ".repeat(widths[*i].saturating_sub(text.width()));
        let last = k + 1 == cells.len();
        let shown = (cell.style)(&text);
        match self.align[*i] {
          Align::Right => line.push_str(&format!("{pad}{shown}")),
          Align::Left if last => line.push_str(&shown),
          Align::Left => line.push_str(&format!("{shown}{pad}")),
        }
        if !last {
          line.push_str("  ");
        }
      }
      println!("{}", line.trim_end());
    }
  }
}

/// Text cut to a width, with "…" where it was cut.
pub fn cut(s: &str, width: usize) -> String {
  if s.width() <= width {
    return s.to_string();
  }
  let mut out = String::new();
  for ch in s.chars() {
    let next = format!("{out}{ch}");
    if next.width() + 1 > width {
      break;
    }
    out = next;
  }
  out.push('…');
  out
}

/// A bar of up to `width` blocks for `part` of `whole`.
pub fn bar(part: i64, whole: i64, width: usize) -> String {
  if whole <= 0 {
    return String::new();
  }
  let eighths = ((part.unsigned_abs() as f64 / whole.unsigned_abs() as f64) * (width * 8) as f64).round() as usize;
  let full = eighths / 8;
  let rest = eighths % 8;
  let mut s = "█".repeat(full);
  if rest > 0 {
    s.push(['▏', '▎', '▍', '▌', '▋', '▊', '▉'][rest - 1]);
  }
  blue(&s)
}

#[cfg(windows)]
mod os {
  use super::Lang;
  use windows::Win32::System::Console::{
    CONSOLE_MODE, CONSOLE_SCREEN_BUFFER_INFO, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetConsoleScreenBufferInfo, GetStdHandle, STD_ERROR_HANDLE,
    STD_OUTPUT_HANDLE, SetConsoleMode,
  };

  /// Windows 10 and later understand colour codes once asked to.
  pub fn enable_ansi() -> bool {
    let mut ok = false;
    for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
      unsafe {
        let Ok(handle) = GetStdHandle(which) else { continue };
        let mut mode = CONSOLE_MODE::default();
        if GetConsoleMode(handle, &mut mode).is_ok() {
          let on = SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING).is_ok();
          if which == STD_OUTPUT_HANDLE {
            ok = on;
          }
        }
      }
    }
    ok
  }

  pub fn terminal_width() -> Option<usize> {
    unsafe {
      let handle = GetStdHandle(STD_OUTPUT_HANDLE).ok()?;
      let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
      GetConsoleScreenBufferInfo(handle, &mut info).ok()?;
      let w = (info.srWindow.Right - info.srWindow.Left + 1) as usize;
      (w > 0).then_some(w)
    }
  }

  /// The language of Windows' own texts.
  pub fn system_lang() -> Option<Lang> {
    let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    // The primary language is the low 10 bits; 0x07 is German.
    Some(if id & 0x3ff == 0x07 { Lang::De } else { Lang::En })
  }
}

#[cfg(unix)]
mod os {
  use super::Lang;

  pub fn enable_ansi() -> bool {
    true
  }

  pub fn terminal_width() -> Option<usize> {
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } == 0;
    (ok && size.ws_col > 0).then_some(size.ws_col as usize)
  }

  pub fn system_lang() -> Option<Lang> {
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn amounts() {
    let _ = LANG.set(Lang::De);
    assert_eq!(cents(123456, "EUR"), "1.234,56 €");
    assert_eq!(cents(-5, "EUR"), "-0,05 €");
    assert_eq!(cents(100000000, "USD"), "1.000.000,00 USD");
    assert_eq!(day("2026-11-15"), "15.11.2026");
    assert_eq!(month("2026-03"), "März 2026");
    assert_eq!(iban("DE89370400440532013000"), "DE89 3704 0044 0532 0130 00");
    assert_eq!(cut("Supermarkt", 6), "Super…");
  }
}
