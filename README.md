# Girovo CLI

Your accounts and bookings from [Girovo](https://girovo.de) in the terminal.
Girovo CLI is a small example app for the Girovo API: it talks to the Girovo
that runs on your computer, and Girovo decides what it may do.

```
$ girovo-cli accounts
Konten
  Konto                  IBAN                         Kontostand   Verfügbar
  Girokonto · Sparkasse  DE89 3704 0044 0532 0130 00  1.234,56 €  2.234,56 €
  Tagesgeld · DKB        DE02 1203 0000 0000 2020 51  8.500,00 €

  Gesamtsaldo  9.734,56 €
```

## What you need

- Girovo 7 (Girovo Next) on Windows or Linux, open and unlocked.
- Girovo API switched on: Einstellungen → Girovo API → Girovo API zulassen.

The first time a command needs something, Girovo asks you in its own window
which permissions Girovo CLI gets. You can take any of them back later in
Einstellungen → Girovo API. Transfers are only suggested: Girovo opens the
transfer filled in, and you check and send it there.

## Install

Windows: download `girovo-cli-<version>-windows-x64.exe` from
[Releases](https://github.com/elNino0916/GirovoCLI/releases), rename it to
`girovo-cli.exe` and put it in a folder on your `PATH`. It is signed by
MT-Media, so Girovo shows it as "Girovo CLI" from MT-Media.

Girovo keeps your answer for that file in that place. If you move the file,
Girovo asks again.

Linux, or from source (Rust 1.88 or newer):

```
cargo build --release
./target/release/girovo-cli --help
```

On Linux programs carry no signature, so Girovo shows Girovo CLI by its path
and asks again whenever the file changes.

## Commands

| Command | What it does | Permission |
|---|---|---|
| `status` | Girovo's version and what Girovo CLI may do | none |
| `accounts` | Accounts with balance, available amount and the total | Konten und Salden lesen |
| `transactions [search]` | Bookings, newest first. `-a <account>`, `--from`/`--to <yyyy-mm-dd>`, `-c <category>`, `-n <count>`, `--all` | Umsätze lesen |
| `month [yyyy-mm]` | Income, spending and both by category | Analyse, Verträge, Budgets und Sparziele lesen |
| `budgets [yyyy-mm]` | Monthly budgets against spending | same |
| `contracts` | Contracts and subscriptions | same |
| `goals` | Savings goals | same |
| `category <t_…> <category\|->` | Files a booking under a category; `-` puts it back | Kategorien ändern |
| `rule <t_…\|payee> <category>` | Files every booking of a payee that way from now on | Kategorien ändern |
| `note <t_…> [text]` | Sets a note on a booking; no text removes it | Notizen schreiben |
| `settings [name]` | Girovo's settings an app may see | Einstellungen lesen |
| `set <name> <value>` | Changes one of them (`an`/`aus`, numbers, lists as `a,b`) | Einstellungen ändern |
| `refresh [account]` | Gets new bookings from the bank | Neue Umsätze von der Bank holen |
| `transfer <IBAN> <amount> --name <payee>` | Suggests a transfer and waits until you sent or closed it in Girovo. `--purpose`, `--on <yyyy-mm-dd>`, `--from <account>` | Überweisungen vorschlagen |
| `watch [--kinds a,b]` | Prints new bookings and balances as Girovo sees them (`bookings.new`, `balances.changed`, `transfer.updated`) | per kind |
| `call <method> [json]` | Any Girovo API method, the answer as JSON. `--permissions a,b` asks for those first | per method |

An account can be given by its name, the end of its IBAN or its id (`a_…`).
Bookings are named by the id in the last column of `transactions`. Other
tables leave ids out; `--ids` shows them. A column with nothing in it, such as
Verfügbar when no bank reports it, is left out.

`--json` prints Girovo's own answer instead of a table, `--no-color` (or
`NO_COLOR`) leaves colours out. Texts are German when your system is, else
English; `GIROVO_LANG=de` or `en` picks one.

Exit codes: `0` done, `1` Girovo said no or a transfer didn't go through,
`2` wrong command line, `3` Girovo isn't open or Girovo API is off, `4` you
didn't allow it in Girovo.

## How it talks to Girovo

Girovo listens on a named pipe, `\\.\pipe\girovo-api-<your SID>`, on Windows,
and on a Unix socket, `$XDG_RUNTIME_DIR/girovo-api.sock`, on Linux. Nothing
listens on the network. Messages are JSON-RPC 2.0, one per line, and every
connection starts with `girovo.hello`. Girovo CLI checks that the other end
runs as your own user before it sends anything. `GIROVO_API_ADDRESS` points it
somewhere else, for example at a test server.

The code is short on purpose: [src/girovo.rs](src/girovo.rs) is the
connection, and [src/commands.rs](src/commands.rs) has one function per
command. The full API reference is on
[developer.girovo.de](https://developer.girovo.de).

## Releases

There is no CI. A release is built and signed by hand on Windows, where the
MT-Media certificate is in the user's certificate store:

```
pwsh -File scripts/release.ps1            # test, build, sign into dist/
pwsh -File scripts/release.ps1 -Publish   # the same, then the GitHub release v<version>
```

[scripts/sign.ps1](scripts/sign.ps1) signs with SignTool (SHA-256, RFC 3161
timestamp) and checks the signer's thumbprint afterwards.

## License

Girovo CLI's source is available under the
[PolyForm Perimeter License 1.0.1](LICENSE)
(<https://polyformproject.org/licenses/perimeter/1.0.1>), like Girovo's.
