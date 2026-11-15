// The commands: each one or a few Girovo API methods, printed as a table or,
// with --json, as Girovo's own answer.

use serde_json::{Value, json};

use crate::Args;
use crate::exit;
use crate::girovo::{Error, Girovo};
use crate::show::{self, Align, Cell, Table, bold, dim, heading, money, money_signed, str_of, tr};

/// What went wrong, and the exit code for it.
enum Fail {
  Girovo(Error),
  Usage(String),
  /// Said already; only the exit code is left.
  Code(u8),
}

impl From<Error> for Fail {
  fn from(e: Error) -> Fail {
    Fail::Girovo(e)
  }
}

type Done = Result<(), Fail>;

fn usage(de: &str, en: &str) -> Fail {
  Fail::Usage(if show::lang() == show::Lang::De { de.to_string() } else { en.to_string() })
}

/// The line Girovo's dialog shows as this app's own words.
fn reason() -> &'static str {
  tr("Für Befehle, die du im Terminal eingibst.", "For commands you type in the terminal.")
}

struct Ctx<'a> {
  g: Girovo,
  args: &'a Args,
}

impl Ctx<'_> {
  fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
    self.g.call(method, params, reason())
  }

  /// With --json, Girovo's answer as it came; true when it was printed.
  fn json(&self, v: &Value) -> bool {
    if self.args.json {
      println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    }
    self.args.json
  }
}

/// The commands, their other names, and the permissions their first hello asks for.
const COMMANDS: &[(&str, &[&str], &[&str])] = &[
  ("status", &[], &[]),
  ("accounts", &["konten"], &["accounts.read"]),
  ("transactions", &["umsaetze", "umsätze", "tx"], &["transactions.read"]),
  ("month", &["monat"], &["insights.read"]),
  ("budgets", &[], &["insights.read"]),
  ("contracts", &["vertraege", "verträge"], &["insights.read"]),
  ("goals", &["sparziele"], &["insights.read"]),
  ("category", &["kategorie"], &["categories.write"]),
  ("rule", &["regel"], &["categories.write"]),
  ("note", &["notiz"], &["notes.write"]),
  ("settings", &["einstellungen"], &["settings.read"]),
  ("set", &["setzen"], &["settings.write"]),
  ("refresh", &["aktualisieren"], &["bank.refresh", "accounts.read"]),
  ("transfer", &["ueberweisen", "überweisen"], &["transfers.request"]),
  ("watch", &["beobachten"], &[]),
  ("call", &[], &[]),
];

pub fn run(args: &Args) -> u8 {
  let Some(&(name, _, permissions)) = COMMANDS
    .iter()
    .find(|(n, other, _)| *n == args.command || other.contains(&args.command.as_str()))
  else {
    eprintln!("{} {}", tr("Unbekannter Befehl:", "Unknown command:"), args.command);
    eprintln!("{}", tr("Hilfe: girovo-cli --help", "Help: girovo-cli --help"));
    return exit::USAGE;
  };
  match start(args, name, permissions) {
    Ok(()) => exit::OK,
    Err(Fail::Code(c)) => c,
    Err(Fail::Usage(text)) => {
      eprintln!("{text}");
      exit::USAGE
    }
    Err(Fail::Girovo(e)) => {
      eprintln!("{}", explain(&e));
      match e {
        Error::NotRunning | Error::NotGirovo => exit::NOT_RUNNING,
        _ if matches!(e.reason(), "refused_by_user" | "denied" | "not_permitted" | "timeout") => exit::NOT_ALLOWED,
        _ => exit::FAILED,
      }
    }
  }
}

fn start(args: &Args, name: &str, permissions: &[&str]) -> Done {
  let mut permissions: Vec<&str> = permissions.to_vec();
  // What else this call will need, asked in the same dialog.
  match name {
    "watch" => permissions.extend(watch_kinds(args)?.iter().map(|k| permission_of_kind(k))),
    "call" => permissions.extend(args.option(&["permissions"]).unwrap_or("").split(',').map(str::trim).filter(|p| !p.is_empty())),
    _ => {}
  }
  if needs_account_names(args, name) {
    permissions.push("accounts.read");
  }
  permissions.sort();
  permissions.dedup();

  let mut g = Girovo::connect(args.address.as_deref())?;
  g.on_waiting(|| {
    eprintln!(
      "{}",
      dim(tr("Girovo fragt dich gerade. Antworte in Girovo.", "Girovo is asking you. Answer in Girovo."))
    )
  });
  g.hello(&permissions, reason())?;
  let mut ctx = Ctx { g, args };
  match name {
    "status" => status(&mut ctx),
    "accounts" => accounts(&mut ctx),
    "transactions" => transactions(&mut ctx),
    "month" => month(&mut ctx),
    "budgets" => budgets(&mut ctx),
    "contracts" => contracts(&mut ctx),
    "goals" => goals(&mut ctx),
    "category" => category(&mut ctx),
    "rule" => rule(&mut ctx),
    "note" => note(&mut ctx),
    "settings" => settings(&mut ctx),
    "set" => set(&mut ctx),
    "refresh" => refresh(&mut ctx),
    "transfer" => transfer(&mut ctx),
    "watch" => watch(&mut ctx),
    "call" => raw_call(&mut ctx),
    _ => unreachable!(),
  }
}

/// An account given by name or IBAN needs the account list to find its id.
fn needs_account_names(args: &Args, name: &str) -> bool {
  let by_name = |s: &str| !s.starts_with("a_");
  match name {
    "transactions" => args.option(&["account"]).is_some_and(by_name),
    "transfer" => args.option(&["from"]).is_some_and(by_name),
    "refresh" => args.words.first().is_some_and(|s| by_name(s)),
    _ => false,
  }
}

// ---------------------------------------------------------------------------
// What Girovo's errors mean
// ---------------------------------------------------------------------------

pub fn permission_label(p: &str) -> String {
  let (de, en) = match p {
    "accounts.read" => ("Konten und Salden lesen", "Read accounts and balances"),
    "transactions.read" => ("Umsätze lesen", "Read transactions"),
    "insights.read" => (
      "Analyse, Verträge, Budgets und Sparziele lesen",
      "Read analysis, contracts, budgets and savings goals",
    ),
    "ask" => ("Girovo AI fragen", "Ask Girovo AI"),
    "categories.write" => ("Kategorien ändern", "Change categories"),
    "notes.write" => ("Notizen schreiben", "Write notes"),
    "budgets.write" => ("Budgets und Sparziele ändern", "Change budgets and savings goals"),
    "settings.read" => ("Einstellungen lesen", "Read settings"),
    "settings.write" => ("Einstellungen ändern", "Change settings"),
    "bank.refresh" => ("Neue Umsätze von der Bank holen", "Get new transactions from the bank"),
    "transfers.request" => ("Überweisungen vorschlagen", "Suggest transfers"),
    other => return other.to_string(),
  };
  tr(de, en).to_string()
}

fn explain(e: &Error) -> String {
  let lang_de = show::lang() == show::Lang::De;
  let pick = |de: &str, en: &str| if lang_de { de.to_string() } else { en.to_string() };
  match e {
    Error::NotRunning => pick(
      "Girovo ist nicht geöffnet oder Girovo API ist aus. Schalte sie in Einstellungen → Girovo API ein.",
      "Girovo isn't open or Girovo API is off. Turn it on in Settings → Girovo API.",
    ),
    Error::NotGirovo => pick(
      "An Girovos Adresse antwortet ein Programm eines anderen Benutzers. Girovo CLI spricht nicht mit ihm.",
      "A program of another user answers at Girovo's address. Girovo CLI won't talk to it.",
    ),
    Error::Refused { reason, message } => {
      let text = match reason.as_str() {
        "busy" => pick(
          "Girovo hat gerade zu viele Verbindungen. Versuch es gleich noch mal.",
          "Girovo has too many connections right now. Try again shortly.",
        ),
        "denied" => pick(
          "Girovo konnte nicht erkennen, welches Programm das ist.",
          "Girovo couldn't tell which program this is.",
        ),
        "timeout" => pick(
          "Girovo hat die Verbindung beendet. Versuch es noch mal.",
          "Girovo ended the connection. Try again.",
        ),
        _ => pick("Girovo hat die Verbindung abgelehnt.", "Girovo turned the connection away."),
      };
      if message.is_empty() {
        text
      } else {
        format!("{text} {}", dim(&format!("({message})")))
      }
    }
    Error::Rpc { reason, message, data } => {
      let detail = data.get("detail").and_then(Value::as_str).unwrap_or("");
      let param = data.get("param").and_then(Value::as_str).unwrap_or("");
      match reason.as_str() {
        "locked" => pick(
          "Girovo ist gesperrt. Melde dich in Girovo an und versuch es noch mal.",
          "Girovo is locked. Log in to Girovo and try again.",
        ),
        "denied" => pick(
          "Du hast Girovo CLI in Girovo abgelehnt. Bis Girovo neu startet, fragt es nicht noch mal.",
          "You refused Girovo CLI in Girovo. Girovo won't ask again until it restarts.",
        ),
        "refused_by_user" => match data.get("permission").and_then(Value::as_str) {
          Some(p) => format!(
            "{} „{}“.",
            pick("Du hast Girovo CLI das in Girovo nicht erlaubt:", "You didn't allow this in Girovo:"),
            permission_label(p)
          ),
          None => pick("Du hast das in Girovo nicht erlaubt.", "You didn't allow this in Girovo."),
        },
        "not_permitted" => {
          let p = data.get("permission").and_then(Value::as_str).unwrap_or("");
          format!(
            "{} „{}“. {}",
            pick("Girovo CLI darf das nicht:", "Girovo CLI may not do this:"),
            permission_label(p),
            pick(
              "Du kannst es in Einstellungen → Girovo API erlauben.",
              "You can allow it in Settings → Girovo API."
            )
          )
        }
        "timeout" => pick("In Girovo kam keine Antwort. Versuch es noch mal.", "No answer came in Girovo. Try again."),
        "bank_error" => {
          let text = data.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()).unwrap_or(message);
          format!("{} {text}", pick("Die Bank sagt:", "The bank says:"))
        }
        "method_not_found" => pick(
          "Diese Girovo-Version kennt das noch nicht. Aktualisiere Girovo.",
          "This Girovo version doesn't know this yet. Update Girovo.",
        ),
        "invalid" => invalid_text(param, detail, data, lang_de),
        _ => {
          let base = pick("In Girovo ist etwas schiefgegangen.", "Something went wrong in Girovo.");
          if message.is_empty() {
            base
          } else {
            format!("{base} {}", dim(&format!("({message})")))
          }
        }
      }
    }
  }
}

fn invalid_text(param: &str, detail: &str, data: &Value, de: bool) -> String {
  let pick = |d: &str, e: &str| if de { d.to_string() } else { e.to_string() };
  // The finer reason: data.detail from Girovo's page, or data.reason beside a param.
  let why = if detail.is_empty() {
    data.get("reason").and_then(Value::as_str).filter(|r| *r != "invalid").unwrap_or("")
  } else {
    detail
  };
  match why {
    "never" => pick("Das lässt Girovo nie von einer App ändern.", "Girovo never lets an app change this."),
    "deletes_data" => pick(
      "Ausschalten löscht gespeicherte Daten. Das geht nur in Girovo selbst.",
      "Turning this off deletes saved data. Only Girovo itself can do that.",
    ),
    "consent" => pick(
      "Einschalten braucht deine Zustimmung in Girovo selbst.",
      "Turning this on needs your consent in Girovo itself.",
    ),
    "unavailable" => pick("Diese Einstellung gibt es hier gerade nicht.", "This setting isn't available here right now."),
    "busy" => pick(
      "In Girovo warten schon zu viele Überweisungen.",
      "Too many transfers are already waiting in Girovo.",
    ),
    "plus" => pick("Das gibt es nur mit Girovo Plus.", "This needs Girovo Plus."),
    "not_offered" => pick(
      "Dein Konto bietet keine Terminüberweisung an.",
      "Your account doesn't offer scheduled transfers.",
    ),
    "same_account" => pick(
      "Das ist das Konto, von dem die Überweisung kommt.",
      "That's the account the transfer comes from.",
    ),
    _ => {
      let what = if param.is_empty() { "params" } else { param };
      format!("{} {what}", pick("Girovo nimmt das so nicht an:", "Girovo won't take this:"))
    }
  }
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

fn status(ctx: &mut Ctx) -> Done {
  let granted: Vec<String> = ctx.g.granted().iter().cloned().collect();
  if ctx.json(&json!({ "girovo": ctx.g.version, "granted": granted })) {
    return Ok(());
  }
  let version = if ctx.g.version.is_empty() {
    String::new()
  } else {
    format!(" {}", ctx.g.version)
  };
  println!("{}", heading(&format!("Girovo{version}")));
  if granted.is_empty() {
    println!(
      "{}",
      tr(
        "Girovo CLI darf noch nichts. Jeder Befehl fragt dich in Girovo, was er braucht.",
        "Girovo CLI may do nothing yet. Each command asks you in Girovo for what it needs."
      )
    );
    return Ok(());
  }
  println!("{}", tr("Girovo CLI darf:", "Girovo CLI may:"));
  for p in granted {
    println!("  {}", permission_label(&p));
  }
  Ok(())
}

// ---------------------------------------------------------------------------
// accounts
// ---------------------------------------------------------------------------

fn accounts(ctx: &mut Ctx) -> Done {
  let r = ctx.call("girovo.accounts.list", json!({}))?;
  if ctx.json(&r) {
    return Ok(());
  }
  let list = r["accounts"].as_array().cloned().unwrap_or_default();
  if list.is_empty() {
    println!("{}", tr("Girovo hat noch keine Konten geladen.", "Girovo hasn't loaded any accounts yet."));
    return Ok(());
  }
  println!("{}", heading(tr("Konten", "Accounts")));
  let mut t = Table::new(&[Align::Left, Align::Left, Align::Right, Align::Right, Align::Left], Some(0));
  t.row(vec![
    Cell::styled(tr("Konto", "Account"), dim),
    Cell::styled("IBAN", dim),
    Cell::styled(tr("Kontostand", "Balance"), dim),
    Cell::styled(tr("Verfügbar", "Available"), dim),
    Cell::new(""),
  ]);
  for a in &list {
    let name = format!("{} · {}", str_of(a, "name"), str_of(a, "bank"));
    t.row(vec![
      Cell::new(name),
      Cell::new(a.get("iban").and_then(Value::as_str).map(show::iban).unwrap_or_default()),
      Cell::styled(money(&a["balance"]), bold),
      Cell::new(money(&a["available"])),
      Cell::styled(str_of(a, "id"), dim),
    ]);
  }
  t.print();
  for total in r["total"].as_array().into_iter().flatten() {
    println!("\n  {}  {}", tr("Gesamtsaldo", "Total balance"), bold(&money(total)));
  }
  Ok(())
}

/// An account's id from its id, its name or the end of its IBAN.
fn account_id(ctx: &mut Ctx, given: &str) -> Result<String, Fail> {
  if given.starts_with("a_") {
    return Ok(given.to_string());
  }
  let r = ctx.call("girovo.accounts.list", json!({}))?;
  let list = r["accounts"].as_array().cloned().unwrap_or_default();
  let low = given.to_lowercase();
  let compact: String = given.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
  let exact: Vec<&Value> = list.iter().filter(|a| str_of(a, "name").to_lowercase() == low).collect();
  let found: Vec<&Value> = if !exact.is_empty() {
    exact
  } else {
    list
      .iter()
      .filter(|a| {
        str_of(a, "name").to_lowercase().contains(&low)
          || (compact.len() >= 4
            && a
              .get("iban")
              .and_then(Value::as_str)
              .is_some_and(|i| i.replace(' ', "").to_uppercase().ends_with(&compact)))
      })
      .collect()
  };
  match found.as_slice() {
    [one] => Ok(str_of(one, "id").to_string()),
    [] => Err(usage(
      &format!("Kein Konto passt zu „{given}“. Siehe girovo-cli accounts."),
      &format!("No account matches \"{given}\". See girovo-cli accounts."),
    )),
    many => {
      let names: Vec<&str> = many.iter().map(|a| str_of(a, "name")).collect();
      Err(usage(
        &format!("Mehrere Konten passen: {}.", names.join(", ")),
        &format!("More than one account matches: {}.", names.join(", ")),
      ))
    }
  }
}

/// Account names by id, for lines that only carry the id.
fn account_names(ctx: &mut Ctx) -> Vec<(String, String)> {
  if !ctx.g.granted().contains("accounts.read") {
    return Vec::new();
  }
  ctx
    .call("girovo.accounts.list", json!({}))
    .ok()
    .and_then(|r| r["accounts"].as_array().cloned())
    .unwrap_or_default()
    .iter()
    .map(|a| (str_of(a, "id").to_string(), str_of(a, "name").to_string()))
    .collect()
}

fn name_of(names: &[(String, String)], id: &str) -> String {
  names.iter().find(|(i, _)| i == id).map(|(_, n)| n.clone()).unwrap_or_else(|| id.to_string())
}

// ---------------------------------------------------------------------------
// transactions
// ---------------------------------------------------------------------------

fn transactions(ctx: &mut Ctx) -> Done {
  let a = ctx.args;
  let mut params = json!({});
  if let Some(acc) = a.option(&["account"]) {
    params["account"] = json!(account_id(ctx, acc)?);
  }
  for (opt, key) in [("from", "from"), ("to", "to"), ("query", "query"), ("category", "category")] {
    if let Some(v) = a.option(&[opt]) {
      params[key] = json!(v);
    }
  }
  // Words after the command are a search, as in Girovo's own search field.
  if !a.words.is_empty() && params.get("query").is_none() {
    params["query"] = json!(a.words.join(" "));
  }
  let all = a.flag(&["all"]);
  let limit: u64 = match a.option(&["limit"]) {
    Some(n) => n
      .parse()
      .ok()
      .filter(|n| (1..=500).contains(n))
      .ok_or_else(|| usage("--limit nimmt 1 bis 500.", "--limit takes 1 to 500."))?,
    None if all => 500,
    None => 30,
  };
  params["limit"] = json!(limit);

  let mut rows: Vec<Value> = Vec::new();
  loop {
    let r = ctx.call("girovo.transactions.list", params.clone())?;
    rows.extend(r["transactions"].as_array().cloned().unwrap_or_default());
    match r.get("next").and_then(Value::as_str) {
      Some(next) if all => params["cursor"] = json!(next),
      _ => break,
    }
  }
  if ctx.json(&json!({ "transactions": rows })) {
    return Ok(());
  }
  if rows.is_empty() {
    println!("{}", tr("Keine Umsätze gefunden.", "No transactions found."));
    return Ok(());
  }
  let mut t = Table::new(&[Align::Left, Align::Right, Align::Left, Align::Left, Align::Left], Some(2));
  for tx in &rows {
    let mut who = str_of(&tx["counterparty"], "name").to_string();
    if who.is_empty() {
      who = str_of(tx, "purpose").to_string();
    }
    if let Some(note) = tx.get("note").and_then(Value::as_str) {
      who = format!("{who} · {note}");
    }
    t.row(vec![
      Cell::new(show::day(str_of(tx, "day"))),
      Cell::styled(
        money(&tx["amount"]),
        if tx["amount"]["cents"].as_i64().unwrap_or(0) > 0 { show::green } else { same },
      ),
      Cell::new(who),
      Cell::styled(str_of(&tx["category"], "name"), dim),
      Cell::styled(str_of(tx, "id"), dim),
    ]);
  }
  t.print();
  if !all && rows.len() as u64 == limit {
    println!("{}", dim(tr("\n  Mehr mit --all oder --limit.", "\n  More with --all or --limit.")));
  }
  Ok(())
}

fn same(s: &str) -> String {
  s.to_string()
}

// ---------------------------------------------------------------------------
// month, budgets, contracts, goals
// ---------------------------------------------------------------------------

fn month_param(ctx: &Ctx) -> Result<Value, Fail> {
  match ctx.args.words.first() {
    Some(m) if m.len() == 7 && m.as_bytes()[4] == b'-' => Ok(json!({ "month": m })),
    Some(_) => Err(usage(
      "Den Monat bitte als JJJJ-MM, zum Beispiel 2026-11.",
      "Give the month as YYYY-MM, for example 2026-11.",
    )),
    None => Ok(json!({})),
  }
}

fn coverage_note(c: &str) -> &'static str {
  match c {
    "running" => tr("läuft noch", "still running"),
    "partial" => tr("nicht ganz geladen", "not fully loaded"),
    "none" => tr("nicht geladen", "not loaded"),
    _ => "",
  }
}

fn month(ctx: &mut Ctx) -> Done {
  let params = month_param(ctx)?;
  let r = ctx.call("girovo.insights.month", params)?;
  if ctx.json(&r) {
    return Ok(());
  }
  let note = coverage_note(str_of(&r, "coverage"));
  let title = show::month(str_of(&r, "month"));
  println!(
    "{}{}",
    heading(&title),
    if note.is_empty() { String::new() } else { dim(&format!("  ({note})")) }
  );
  let mut t = Table::new(&[Align::Left, Align::Right], None);
  t.row(vec![Cell::new(tr("Einnahmen", "Income")), Cell::styled(money(&r["income"]), show::green)]);
  t.row(vec![Cell::new(tr("Ausgaben", "Spending")), Cell::new(money(&r["expense"]))]);
  t.row(vec![Cell::styled(tr("Saldo", "Net"), bold), Cell::styled(money(&r["net"]), bold)]);
  t.print();
  for (key, de, en) in [
    ("expenseByCategory", "Ausgaben nach Kategorie", "Spending by category"),
    ("incomeByCategory", "Einnahmen nach Kategorie", "Income by category"),
  ] {
    let rows = r[key].as_array().cloned().unwrap_or_default();
    if rows.is_empty() {
      continue;
    }
    println!("\n{}", heading(tr(de, en)));
    let top = rows.iter().map(|c| c["amount"]["cents"].as_i64().unwrap_or(0).abs()).max().unwrap_or(0);
    let mut t = Table::new(&[Align::Left, Align::Right, Align::Left, Align::Left], Some(0));
    for c in &rows {
      let cents = c["amount"]["cents"].as_i64().unwrap_or(0);
      t.row(vec![
        Cell::new(str_of(&c["category"], "name")),
        Cell::new(money(&c["amount"])),
        Cell::new(show::bar(cents, top, 16)),
        Cell::styled(str_of(&c["category"], "id"), dim),
      ]);
    }
    t.print();
  }
  if let Some(n) = r["transfersLeftOut"].as_u64().filter(|n| *n > 0) {
    let text = if show::lang() == show::Lang::De {
      format!("{n} Umbuchungen zwischen deinen Konten sind nicht mitgezählt.")
    } else {
      format!("{n} transfers between your accounts aren't counted.")
    };
    println!("\n  {}", dim(&text));
  }
  Ok(())
}

fn budgets(ctx: &mut Ctx) -> Done {
  let params = month_param(ctx)?;
  let r = ctx.call("girovo.budgets.list", params)?;
  if ctx.json(&r) {
    return Ok(());
  }
  let rows = r["budgets"].as_array().cloned().unwrap_or_default();
  println!(
    "{}  {}",
    heading(tr("Monatsbudgets", "Monthly budgets")),
    dim(&show::month(str_of(&r, "month")))
  );
  if rows.is_empty() {
    println!("{}", tr("Du hast noch keine Budgets.", "You have no budgets yet."));
    return Ok(());
  }
  let mut t = Table::new(&[Align::Left, Align::Right, Align::Right, Align::Right, Align::Left], Some(0));
  t.row(vec![
    Cell::styled(tr("Kategorie", "Category"), dim),
    Cell::styled(tr("Ausgegeben", "Spent"), dim),
    Cell::styled(tr("Budget", "Budget"), dim),
    Cell::styled(tr("Rest", "Left"), dim),
    Cell::new(""),
  ]);
  for b in &rows {
    let (word, style): (&str, fn(&str) -> String) = match str_of(b, "state") {
      "over" => (tr("drüber", "over"), show::red),
      "exact" => (tr("genau", "exact"), same),
      "unknown" => (tr("offen", "open"), dim),
      _ => (tr("übrig", "left"), show::green),
    };
    t.row(vec![
      Cell::new(str_of(&b["category"], "name")),
      Cell::new(money(&b["spent"])),
      Cell::new(money(&b["budget"])),
      Cell::new(money(&b["rest"])),
      Cell::styled(word, style),
    ]);
  }
  t.print();
  Ok(())
}

fn cadence(c: &str) -> &'static str {
  match c {
    "weekly" => tr("wöchentlich", "weekly"),
    "monthly" => tr("monatlich", "monthly"),
    "quarterly" => tr("vierteljährlich", "quarterly"),
    "halfyearly" => tr("halbjährlich", "every six months"),
    "yearly" => tr("jährlich", "yearly"),
    _ => "",
  }
}

fn contracts(ctx: &mut Ctx) -> Done {
  let r = ctx.call("girovo.contracts.list", json!({}))?;
  if ctx.json(&r) {
    return Ok(());
  }
  let rows = r["contracts"].as_array().cloned().unwrap_or_default();
  println!("{}", heading(tr("Verträge & Abos", "Contracts & subscriptions")));
  if rows.is_empty() {
    println!("{}", tr("Girovo hat noch keine Verträge gefunden.", "Girovo hasn't found any contracts yet."));
    return Ok(());
  }
  let mut t = Table::new(&[Align::Left, Align::Right, Align::Left, Align::Right, Align::Left, Align::Left], Some(0));
  t.row(vec![
    Cell::styled(tr("Name", "Name"), dim),
    Cell::styled(tr("Betrag", "Amount"), dim),
    Cell::new(""),
    Cell::styled(tr("pro Monat", "per month"), dim),
    Cell::styled(tr("Nächster", "Next"), dim),
    Cell::new(""),
  ]);
  let mut month_total = 0i64;
  let mut currency = String::from("EUR");
  for c in &rows {
    let mut name = str_of(c, "name").to_string();
    if c.get("priceChange").is_some() {
      name.push_str(tr(" (neuer Preis)", " (new price)"));
    }
    let per = c["perMonth"]["cents"].as_i64().unwrap_or(0);
    if str_of(c, "kind") != "income" {
      month_total += per;
      currency = str_of(&c["perMonth"], "currency").to_string();
    }
    t.row(vec![
      Cell::new(name),
      Cell::new(money(&c["amount"])),
      Cell::styled(cadence(str_of(c, "cadence")), dim),
      Cell::new(money(&c["perMonth"])),
      Cell::new(c.get("nextDay").and_then(Value::as_str).map(show::day).unwrap_or_default()),
      Cell::styled(str_of(c, "id"), dim),
    ]);
  }
  t.print();
  println!(
    "\n  {}  {}",
    tr("Zusammen pro Monat", "Together per month"),
    bold(&show::cents(month_total, &currency))
  );
  Ok(())
}

fn goals(ctx: &mut Ctx) -> Done {
  let r = ctx.call("girovo.goals.list", json!({}))?;
  if ctx.json(&r) {
    return Ok(());
  }
  let rows = r["goals"].as_array().cloned().unwrap_or_default();
  println!("{}", heading(tr("Sparziele", "Savings goals")));
  if rows.is_empty() {
    println!("{}", tr("Du hast noch keine Sparziele.", "You have no savings goals yet."));
    return Ok(());
  }
  let mut t = Table::new(&[Align::Left, Align::Right, Align::Right, Align::Left, Align::Left], Some(0));
  for goal in &rows {
    let state = if goal.get("reached").and_then(Value::as_bool) == Some(true) {
      show::green(tr("erreicht", "reached"))
    } else if goal.get("missing").is_some() {
      format!("{} {}", tr("fehlt", "missing"), money(&goal["missing"]))
    } else {
      String::new()
    };
    let by = goal
      .get("by")
      .and_then(Value::as_str)
      .map(|b| format!("{} {}", tr("bis", "by"), show::day(b)))
      .unwrap_or_default();
    t.row(vec![
      Cell::new(str_of(goal, "name")),
      Cell::new(money(&goal["balance"])),
      Cell::styled(money(&goal["target"]), bold),
      Cell::styled(by, dim),
      Cell::new(state),
    ]);
  }
  t.print();
  Ok(())
}

// ---------------------------------------------------------------------------
// category, rule, note
// ---------------------------------------------------------------------------

fn category(ctx: &mut Ctx) -> Done {
  let [booking, cat] = ctx.args.words.as_slice() else {
    return Err(usage(
      "So geht es: girovo-cli category <Umsatz> <Kategorie|->",
      "Usage: girovo-cli category <transaction> <category|->",
    ));
  };
  let category = if cat == "-" { Value::Null } else { json!(cat) };
  let r = ctx.call("girovo.categories.set", json!({ "booking": booking, "category": category }))?;
  if ctx.json(&r) {
    return Ok(());
  }
  match r["category"].get("name").and_then(Value::as_str) {
    Some(name) => println!("{} {}", tr("Eingeordnet unter", "Filed under"), bold(name)),
    None => println!("{}", tr("Wieder so eingeordnet, wie Girovo es sieht.", "Back to how Girovo files it.")),
  }
  Ok(())
}

fn rule(ctx: &mut Ctx) -> Done {
  let [who, cat] = ctx.args.words.as_slice() else {
    return Err(usage(
      "So geht es: girovo-cli rule <Umsatz|Empfänger> <Kategorie>",
      "Usage: girovo-cli rule <transaction|payee> <category>",
    ));
  };
  let params = if who.starts_with("t_") {
    json!({ "booking": who, "category": cat })
  } else {
    json!({ "payee": who, "category": cat })
  };
  let r = ctx.call("girovo.rules.add", params)?;
  if ctx.json(&r) {
    return Ok(());
  }
  let payees = r["payees"]
    .as_array()
    .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
    .unwrap_or_default();
  let name = str_of(&r["category"], "name");
  match (r["applied"].as_bool(), str_of(&r, "reason")) {
    (Some(true), _) => println!("{} {} → {}", tr("Ab jetzt immer so:", "From now on:"), payees, bold(name)),
    (_, "filed_by_hand") => println!(
      "{}",
      tr(
        "Du hast diesen Empfänger selbst eingeordnet. Girovo lässt das so.",
        "You filed this payee yourself. Girovo leaves it as it is."
      )
    ),
    (_, "already") => println!("{}", tr("Das ist schon so eingestellt.", "That's already set.")),
    _ => println!("{}", tr("Nichts geändert.", "Nothing changed.")),
  }
  Ok(())
}

fn note(ctx: &mut Ctx) -> Done {
  let Some((booking, text)) = ctx.args.words.split_first() else {
    return Err(usage(
      "So geht es: girovo-cli note <Umsatz> [Text]",
      "Usage: girovo-cli note <transaction> [text]",
    ));
  };
  let text = text.join(" ");
  let value = if text.trim().is_empty() { Value::Null } else { json!(text) };
  let r = ctx.call("girovo.notes.set", json!({ "booking": booking, "text": value }))?;
  if ctx.json(&r) {
    return Ok(());
  }
  match r.get("note").and_then(Value::as_str) {
    Some(n) => println!("{} {}", tr("Notiz:", "Note:"), n),
    None => println!("{}", tr("Notiz gelöscht.", "Note removed.")),
  }
  Ok(())
}

// ---------------------------------------------------------------------------
// settings, set
// ---------------------------------------------------------------------------

fn setting_value(v: &Value) -> String {
  match v {
    Value::Bool(true) => tr("an", "on").to_string(),
    Value::Bool(false) => tr("aus", "off").to_string(),
    Value::String(s) => s.clone(),
    Value::Array(a) => a.iter().map(setting_value).collect::<Vec<_>>().join(", "),
    other => other.to_string(),
  }
}

fn settings(ctx: &mut Ctx) -> Done {
  let r = ctx.call("girovo.settings.get", json!({}))?;
  if ctx.json(&r) {
    return Ok(());
  }
  let only = |key: &str, list: &str| r[list].as_array().is_some_and(|a| a.iter().any(|k| k.as_str() == Some(key)));
  let map = r["settings"].as_object().cloned().unwrap_or_default();
  let wanted = ctx.args.words.first();
  println!("{}", heading(tr("Einstellungen", "Settings")));
  let mut t = Table::new(&[Align::Left, Align::Left, Align::Left], Some(1));
  for (key, value) in &map {
    if wanted.is_some_and(|w| w != key) {
      continue;
    }
    let hint = if only(key, "onlyOn") {
      tr("nur einschalten", "can only be turned on")
    } else if only(key, "onlyOff") {
      tr("nur ausschalten", "can only be turned off")
    } else {
      ""
    };
    t.row(vec![Cell::new(key.clone()), Cell::styled(setting_value(value), bold), Cell::styled(hint, dim)]);
  }
  if t.is_empty() {
    return Err(usage("Diese Einstellung gibt es hier nicht.", "There's no such setting here."));
  }
  t.print();
  Ok(())
}

/// "an", "on", "true" are true; numbers are numbers; [..] and {..} are JSON; else text.
fn parse_value(s: &str) -> Value {
  match s.to_lowercase().as_str() {
    "an" | "on" | "true" | "ja" | "yes" => return json!(true),
    "aus" | "off" | "false" | "nein" | "no" => return json!(false),
    _ => {}
  }
  if let Ok(n) = s.parse::<i64>() {
    return json!(n);
  }
  if (s.starts_with('[') || s.starts_with('{'))
    && let Ok(v) = serde_json::from_str(s)
  {
    return v;
  }
  if s.contains(',') && !s.contains(' ') {
    return json!(s.split(',').collect::<Vec<_>>());
  }
  json!(s)
}

fn set(ctx: &mut Ctx) -> Done {
  let [key, value] = ctx.args.words.as_slice() else {
    return Err(usage(
      "So geht es: girovo-cli set <Einstellung> <Wert>",
      "Usage: girovo-cli set <setting> <value>",
    ));
  };
  let r = ctx.call("girovo.settings.set", json!({ "key": key, "value": parse_value(value) }))?;
  if ctx.json(&r) {
    return Ok(());
  }
  let was = if r["changed"].as_bool() == Some(false) {
    dim(tr("  (war schon so)", "  (already set)"))
  } else {
    String::new()
  };
  println!("{} = {}{was}", str_of(&r, "key"), bold(&setting_value(&r["value"])));
  Ok(())
}

// ---------------------------------------------------------------------------
// refresh
// ---------------------------------------------------------------------------

fn refresh(ctx: &mut Ctx) -> Done {
  let mut params = json!({});
  if let Some(acc) = ctx.args.words.first().cloned() {
    params["account"] = json!(account_id(ctx, &acc)?);
  }
  if !ctx.args.json {
    eprintln!("{}", dim(tr("Girovo fragt bei der Bank nach …", "Girovo is asking the bank …")));
  }
  let r = ctx.call("girovo.bank.refresh", params)?;
  if ctx.json(&r) {
    return Ok(());
  }
  let names = account_names(ctx);
  let mut t = Table::new(&[Align::Left, Align::Left, Align::Right], Some(0));
  for a in r["accounts"].as_array().into_iter().flatten() {
    let (state, amount) = match str_of(a, "state") {
      "read" => {
        let n = a["newBookings"].as_u64().unwrap_or(0);
        let text = match (show::lang(), n) {
          (_, 0) => tr("nichts Neues", "nothing new").to_string(),
          (show::Lang::De, 1) => "1 neuer Umsatz".to_string(),
          (show::Lang::De, n) => format!("{n} neue Umsätze"),
          (show::Lang::En, 1) => "1 new transaction".to_string(),
          (show::Lang::En, n) => format!("{n} new transactions"),
        };
        (text, money(&a["balance"]))
      }
      "failed" => (format!("{} {}", tr("ging nicht:", "failed:"), str_of(a, "text")), String::new()),
      "cancelled" => (tr("abgebrochen", "cancelled").to_string(), String::new()),
      other => (other.to_string(), String::new()),
    };
    t.row(vec![
      Cell::new(name_of(&names, str_of(a, "account"))),
      Cell::new(state),
      Cell::styled(amount, bold),
    ]);
  }
  t.print();
  if r["stopped"].as_bool() == Some(true) {
    println!(
      "{}",
      dim(tr(
        "\n  Du hast die Freigabe abgebrochen. Die übrigen Konten sind nicht neu.",
        "\n  You cancelled the approval. The other accounts weren't refreshed."
      ))
    );
  }
  Ok(())
}

// ---------------------------------------------------------------------------
// transfer
// ---------------------------------------------------------------------------

/// "12,34", "12.34", "1.234,56", "1,234.56", "12" as cents; None when it is none of these.
pub fn parse_cents(s: &str) -> Option<i64> {
  let s: String = s.chars().filter(|c| !c.is_whitespace() && *c != '€').collect();
  let last_comma = s.rfind(',');
  let last_dot = s.rfind('.');
  let decimal = match (last_comma, last_dot) {
    (Some(c), Some(d)) => Some(c.max(d)),
    (Some(c), None) => Some(c),
    // A lone dot with three digits after it groups thousands ("1.234").
    (None, Some(d)) if s.len() - d - 1 != 3 => Some(d),
    _ => None,
  };
  let (whole, part) = match decimal {
    Some(i) => (&s[..i], &s[i + 1..]),
    None => (s.as_str(), ""),
  };
  let whole: String = whole.chars().filter(|c| *c != '.' && *c != ',').collect();
  if whole.is_empty() || !whole.chars().all(|c| c.is_ascii_digit()) || part.len() > 2 || !part.chars().all(|c| c.is_ascii_digit()) {
    return None;
  }
  let part = format!("{part:0<2}");
  whole.parse::<i64>().ok()?.checked_mul(100)?.checked_add(part.parse::<i64>().ok()?)
}

fn transfer(ctx: &mut Ctx) -> Done {
  let a = ctx.args;
  let (Some(iban), Some(amount), Some(name)) = (a.words.first(), a.words.get(1), a.option(&["name"])) else {
    return Err(usage(
      "So geht es: girovo-cli transfer <IBAN> <Betrag> --name <Empfänger> [--purpose <Verwendungszweck>] [--on <JJJJ-MM-TT>] [--from <Konto>]",
      "Usage: girovo-cli transfer <IBAN> <amount> --name <payee> [--purpose <reference>] [--on <YYYY-MM-DD>] [--from <account>]",
    ));
  };
  let cents = parse_cents(amount).filter(|c| *c > 0).ok_or_else(|| {
    usage(
      "Den Betrag bitte als Zahl, zum Beispiel 12,50.",
      "Give the amount as a number, for example 12.50.",
    )
  })?;
  let mut params = json!({ "iban": iban.replace(' ', ""), "name": name, "cents": cents });
  if let Some(p) = a.option(&["purpose"]) {
    params["purpose"] = json!(p);
  }
  if let Some(d) = a.option(&["on"]) {
    params["executeOn"] = json!(d);
  }
  if let Some(from) = a.option(&["from"]) {
    params["account"] = json!(account_id(ctx, from)?);
  }
  let r = ctx.call("girovo.transfers.request", params)?;
  let request = str_of(&r, "request").to_string();
  if !ctx.args.json {
    println!(
      "{}",
      tr(
        "Girovo zeigt dir die Überweisung. Prüf sie dort und sende sie ab.",
        "Girovo shows you the transfer. Check it there and send it."
      )
    );
  }
  // The outcome comes as girovo.transfer.updated once you acted in Girovo.
  let outcome = loop {
    let n = ctx.g.notification()?;
    if str_of(&n, "method") == "girovo.transfer.updated" && str_of(&n["params"], "request") == request {
      break n["params"].clone();
    }
  };
  if ctx.json(&outcome) {
    return Ok(());
  }
  let changed = if outcome["changed"].as_bool() == Some(true) {
    tr(" Du hast in Girovo etwas daran geändert.", " You changed something in Girovo.")
  } else {
    ""
  };
  match str_of(&outcome, "state") {
    "sent" => {
      println!(
        "{}{changed}",
        show::green(tr("Die Bank hat die Überweisung angenommen.", "The bank accepted the transfer."))
      );
      Ok(())
    }
    "refused" => {
      println!("{}", tr("Du hast die Überweisung nicht gesendet.", "You didn't send the transfer."));
      Err(Fail::Code(exit::NOT_ALLOWED))
    }
    "unclear" => {
      println!(
        "{}",
        tr(
          "Status unklar. Schau in Girovo nach, ob die Überweisung angekommen ist, bevor du sie noch mal sendest.",
          "Status unclear. Check in Girovo whether the transfer went through before you send it again."
        )
      );
      Err(Fail::Code(exit::FAILED))
    }
    _ => {
      let text = str_of(&outcome, "text");
      println!(
        "{} {text}",
        show::red(tr("Die Überweisung ging nicht durch.", "The transfer didn't go through."))
      );
      Err(Fail::Code(exit::FAILED))
    }
  }
}

// ---------------------------------------------------------------------------
// watch
// ---------------------------------------------------------------------------

const KINDS: [&str; 3] = ["bookings.new", "balances.changed", "transfer.updated"];

fn watch_kinds(args: &Args) -> Result<Vec<String>, Fail> {
  let kinds: Vec<String> = match args.option(&["kinds"]) {
    Some(list) => list.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect(),
    None => vec!["bookings.new".into(), "balances.changed".into()],
  };
  if let Some(bad) = kinds.iter().find(|k| !KINDS.contains(&k.as_str())) {
    return Err(usage(
      &format!("Unbekannte Art: {bad}. Es gibt {}.", KINDS.join(", ")),
      &format!("Unknown kind: {bad}. There are {}.", KINDS.join(", ")),
    ));
  }
  Ok(kinds)
}

fn permission_of_kind(kind: &str) -> &'static str {
  match kind {
    "bookings.new" => "transactions.read",
    "balances.changed" => "accounts.read",
    _ => "transfers.request",
  }
}

fn watch(ctx: &mut Ctx) -> Done {
  let kinds = watch_kinds(ctx.args)?;
  ctx.call("girovo.events.subscribe", json!({ "kinds": kinds }))?;
  let names = account_names(ctx);
  if !ctx.args.json {
    eprintln!(
      "{}",
      dim(tr(
        "Girovo CLI wartet auf Neues aus Girovo. Beenden mit Strg+C.",
        "Girovo CLI is waiting for news from Girovo. Stop with Ctrl+C."
      ))
    );
  }
  loop {
    let n = ctx.g.notification()?;
    if ctx.args.json {
      println!("{}", serde_json::to_string(&n).unwrap_or_default());
      continue;
    }
    let p = &n["params"];
    match str_of(&n, "method") {
      "girovo.bookings.new" => {
        let count = p["count"].as_u64().unwrap_or(0);
        let what = match (show::lang(), count) {
          (show::Lang::De, 1) => "1 neuer Umsatz".to_string(),
          (show::Lang::De, n) => format!("{n} neue Umsätze"),
          (show::Lang::En, 1) => "1 new transaction".to_string(),
          (show::Lang::En, n) => format!("{n} new transactions"),
        };
        println!("{}  {what}", heading(&name_of(&names, str_of(p, "account"))));
        for tx in p["bookings"].as_array().into_iter().flatten() {
          println!(
            "  {}  {}  {}",
            show::day(str_of(tx, "day")),
            money_signed(&tx["amount"]),
            str_of(&tx["counterparty"], "name")
          );
        }
      }
      "girovo.balances.changed" => {
        let available = if p["available"].is_null() {
          String::new()
        } else {
          format!(", {} {}", tr("verfügbar", "available"), money(&p["available"]))
        };
        println!(
          "{}  {} {}{available}",
          heading(&name_of(&names, str_of(p, "account"))),
          tr("Kontostand", "Balance"),
          bold(&money(&p["balance"]))
        );
      }
      "girovo.transfer.updated" => {
        println!("{}  {}", heading(tr("Überweisung", "Transfer")), str_of(p, "state"));
      }
      _ => {}
    }
  }
}

// ---------------------------------------------------------------------------
// call
// ---------------------------------------------------------------------------

fn raw_call(ctx: &mut Ctx) -> Done {
  let Some(method) = ctx.args.words.first() else {
    return Err(usage(
      "So geht es: girovo-cli call <Methode> [JSON] [--permissions a,b]",
      "Usage: girovo-cli call <method> [JSON] [--permissions a,b]",
    ));
  };
  let method = if method.starts_with("girovo.") {
    method.clone()
  } else {
    format!("girovo.{method}")
  };
  let params = match ctx.args.words.get(1) {
    Some(text) => serde_json::from_str(text).map_err(|_| usage("Die Parameter sind kein gültiges JSON.", "The parameters aren't valid JSON."))?,
    None => json!({}),
  };
  let r = ctx.call(&method, params)?;
  println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
  Ok(())
}

// ---------------------------------------------------------------------------
// help
// ---------------------------------------------------------------------------

pub fn help() {
  let de = show::lang() == show::Lang::De;
  println!("{} {}", heading("Girovo CLI"), dim(env!("CARGO_PKG_VERSION")));
  println!(
    "{}\n",
    if de {
      "Deine Konten und Umsätze aus Girovo im Terminal."
    } else {
      "Your accounts and transactions from Girovo in the terminal."
    }
  );
  println!("{}", bold(if de { "So geht es" } else { "Usage" }));
  println!(
    "  girovo-cli <{}> [{}]\n",
    if de { "Befehl" } else { "command" },
    if de { "Optionen" } else { "options" }
  );
  let rows: &[(&str, &str, &str)] = &[
    ("status", "Was Girovo CLI darf", "What Girovo CLI may do"),
    ("accounts", "Konten mit Kontostand und Verfügbar", "Accounts with balance and available amount"),
    ("transactions [Suche]", "Umsätze, neueste zuerst", "Transactions, newest first"),
    ("  -a, --account <Konto>", "Name, Ende der IBAN oder a_…", "Name, end of the IBAN or a_…"),
    ("  --from, --to <Tag>", "Zeitraum, JJJJ-MM-TT", "Date range, YYYY-MM-DD"),
    ("  -c, --category <id>", "Nur diese Kategorie", "Only this category"),
    ("  -n, --limit <n>, --all", "Wie viele (30), oder alle", "How many (30), or all"),
    ("month [JJJJ-MM]", "Einnahmen und Ausgaben eines Monats", "Income and spending of a month"),
    ("budgets [JJJJ-MM]", "Monatsbudgets", "Monthly budgets"),
    ("contracts", "Verträge & Abos", "Contracts & subscriptions"),
    ("goals", "Sparziele", "Savings goals"),
    ("category <t_…> <id|->", "Umsatz einordnen (- setzt zurück)", "File a transaction (- resets)"),
    ("rule <t_…|Empfänger> <id>", "Empfänger immer so einordnen", "Always file a payee this way"),
    ("note <t_…> [Text]", "Notiz setzen oder löschen", "Set or remove a note"),
    ("settings [Name]", "Einstellungen zeigen", "Show settings"),
    ("set <Name> <Wert>", "Eine Einstellung ändern", "Change a setting"),
    ("refresh [Konto]", "Neue Umsätze von der Bank holen", "Get new transactions from the bank"),
    (
      "transfer <IBAN> <Betrag>",
      "Überweisung vorschlagen, du sendest sie in Girovo",
      "Suggest a transfer, you send it in Girovo",
    ),
    (
      "  --name, --purpose, --on, --from",
      "Empfänger, Verwendungszweck, Tag, Konto",
      "Payee, reference, day, account",
    ),
    (
      "watch [--kinds a,b]",
      "Neue Umsätze und Kontostände live",
      "New transactions and balances as they come",
    ),
    ("call <Methode> [JSON]", "Eine Methode der Girovo API direkt", "Any Girovo API method"),
  ];
  println!("{}", bold(if de { "Befehle" } else { "Commands" }));
  let mut t = Table::new(&[Align::Left, Align::Left], Some(1));
  for (cmd, d, e) in rows {
    let cmd = if de {
      cmd.to_string()
    } else {
      cmd
        .replace("Suche", "search")
        .replace("Konto", "account")
        .replace("Tag", "day")
        .replace("JJJJ-MM", "YYYY-MM")
        .replace("Empfänger", "payee")
        .replace("Text", "text")
        .replace("Name", "name")
        .replace("Wert", "value")
        .replace("Betrag", "amount")
        .replace("Methode", "method")
    };
    t.row(vec![Cell::new(cmd), Cell::styled(if de { *d } else { *e }, dim)]);
  }
  t.print();
  println!("\n{}", bold(if de { "Für alle Befehle" } else { "For every command" }));
  let mut t = Table::new(&[Align::Left, Align::Left], Some(1));
  t.row(vec![
    Cell::new("--json"),
    Cell::styled(if de { "Girovos Antwort als JSON" } else { "Girovo's answer as JSON" }, dim),
  ]);
  t.row(vec![Cell::new("--no-color"), Cell::styled(if de { "Ohne Farben" } else { "No colours" }, dim)]);
  t.row(vec![Cell::new("-h, --help  -V, --version"), Cell::new("")]);
  t.print();
  println!(
    "\n{}",
    dim(if de {
      "Girovo muss offen sein, mit Girovo API an (Einstellungen → Girovo API).\nBeim ersten Mal fragt Girovo dich, was Girovo CLI darf. Mehr: developer.girovo.de"
    } else {
      "Girovo must be open with Girovo API on (Settings → Girovo API).\nThe first time, Girovo asks you what Girovo CLI may do. More: developer.girovo.de"
    })
  );
}

#[cfg(test)]
mod tests {
  use super::parse_cents;

  #[test]
  fn amounts() {
    assert_eq!(parse_cents("12,34"), Some(1234));
    assert_eq!(parse_cents("12.34"), Some(1234));
    assert_eq!(parse_cents("12"), Some(1200));
    assert_eq!(parse_cents("1.234,5"), Some(123450));
    assert_eq!(parse_cents("1,234.56"), Some(123456));
    assert_eq!(parse_cents("1.234"), Some(123400));
    assert_eq!(parse_cents("12,345"), None);
    assert_eq!(parse_cents("abc"), None);
    assert_eq!(parse_cents("5 €"), Some(500));
  }
}
