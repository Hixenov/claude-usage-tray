# Claude Usage Monitor v1 — план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Кроссплатформенное (Windows/macOS/Linux) трей-приложение на Rust + Tauri v2, показывающее лимиты подписки Claude (5 ч, неделя, модельные квоты) с попапом, уведомлениями, автозапуском и RU/EN.

**Architecture:** Вся логика — чистые Rust-модули без UI (`model`, `pace`, `credentials`, `api`, `settings`, `i18n`, `poller`, `alerts`, `view`, `tray_icon`), покрытые `cargo test`. Тонкий слой Tauri (`tray`, `popup`, `lib.rs`) связывает их: tokio-задача `poller` обновляет `AppState`, после чего перерисовываются иконка/подсказка, вызываются `alerts` и событие `state` уходит в попап. Попап — статические HTML/CSS/JS без Node.js.

**Tech Stack:** Rust stable, Tauri 2, tauri-plugin-{notification,autostart,single-instance,positioner} 2, reqwest 0.12 (rustls + системные корневые сертификаты через `rustls-tls-native-roots` — заменяет `rustls-platform-verifier` из спеки, тот же эффект без лишней зависимости), serde/serde_json, time 0.3, sys-locale.

**Spec:** `docs/superpowers/specs/2026-11-04-claude-usage-monitor-design.md`

## Global Constraints

- Единственный сетевой хост — `https://api.anthropic.com`; таймаут 10 с; заголовки `Authorization: Bearer <token>`, `anthropic-beta: oauth-2025-04-20`, `User-Agent: claude-usage-monitor/<version>`.
- Токен перечитывается перед каждым запросом; никогда не пишется на диск, в лог или в `Debug`-вывод.
- Приложение не создаёт и не изменяет файл настроек.
- Без Node.js: фронтенд — файлы в `ui/`, `build.frontendDist = "../ui"`, `devUrl` не задаётся.
- Release-профиль: `opt-level = "s"`, `lto = true`, `codegen-units = 1`, `strip = true`, `panic = "abort"`.
- Цели: бинарник/установщик ≤ 15 МБ; ≤ 30 МБ RAM в фоне на Windows; окно попапа уничтожается при закрытии.
- Пороги по умолчанию `[80, 95]`; цвета полос: < 80 синий, ≥ 80 оранжевый, ≥ 95 или «обгоняет темп» красный.
- Минимальный интервал между запросами — 30 с; ручное обновление не чаще раза в 10 с; backoff старт 60 с, ×2, максимум 900 с.
- Все тексты UI — только через `i18n` (RU и EN), никаких захардкоженных строк в `tray`/`popup`/`ui`.
- Git: диск E: не хранит владельцев → каждую git-команду запускать как `git -c safe.directory=E:/claude/claude-usage-monitor …` (если пользователь не добавил исключение глобально). Каждый коммит заканчивается строкой `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Все команды `cargo` запускать из `src-tauri/`.

## Review Focus

1. **Файл токена читается в момент записи Claude Code (обрезанный JSON)** → `CredError::Malformed`, последний удачный снапшот остаётся на экране, показывается ошибка, но квоты не пропадают. Тесты: Task 3 `truncated_json_is_malformed`, Task 9 `error_keeps_last_snapshot`.
2. **`utilization: null`, `resets_at: null`, `utilization` > 100 или < 0** → квота с null-utilization пропускается, проценты клампятся в 0..=100, без `resets_at` нет таймера и маркера. Тесты: Task 4 `null_fields`, Task 9 `percent_is_clamped`.
3. **`resets_at` уже в прошлом (данные устарели после сброса)** → доля периода = 1.0, текст сброса «скоро»/«soon», без отрицательных длительностей. Тесты: Task 2 `fraction_clamped_after_reset`, Task 5 `format_negative_is_soon`.
4. **В ответе API есть не-квотные объекты (`extra_usage` с вложенными полями, пустые объекты, массивы)** → игнорируются; квота только если объект содержит оба ключа `utilization` и `resets_at`. Тест: Task 4 `ignores_non_quota_fields`.
5. **`Retry-After` в формате HTTP-даты или мусор** → трактуется как отсутствующий, применяется экспоненциальный backoff. Тесты: Task 4 `retry_after_garbage_is_none`, Task 7 `backoff_doubles_and_caps`.

---

## File Structure

```
src-tauri/
  Cargo.toml  build.rs  tauri.conf.json  capabilities/default.json  icons/  locales/{ru,en}.json
  src/
    main.rs        — точка входа, вызывает lib::run()
    lib.rs         — сборка Tauri-приложения, плагины, AppState, запуск poller, RunEvent
    model.rs       — Period, Quota, Profile, Snapshot, ApiError, AppState
    pace.rs        — доля прошедшего периода, «обгоняет темп»
    credentials.rs — поиск и разбор токена
    api.rs         — разбор ответов, маппинг статусов, HTTP-клиент
    settings.rs    — Settings + загрузка необязательного JSON
    i18n.rs        — Lang, Strings, format_duration, quota_name
    poller.rs      — next_delay, next_backoff, цикл опроса
    alerts.rs      — Alert, FiredSet, evaluate
    view.rs        — PopupView/QuotaView, tooltip_text (чистые функции для UI)
    tray_icon.rs   — BarColor, render RGBA
    tray.rs        — трей, меню, клики, обновление иконки/подсказки
    popup.rs       — окно попапа + команды get_state/get_strings/refresh
  tests/fixtures/  — usage_full.json, usage_extra.json, profile.json, credentials.json
ui/  index.html  popup.css  popup.js
docs/manual-test.md  README.md  PRIVACY.md  LICENSE
.github/workflows/ci.yml  .github/workflows/release.yml
```

---

### Task 0: Инструменты и каркас Tauri-приложения

Требует разрешения пользователя на установку системных компонентов (выполняет пользователь или агент с его согласия).

**Files:**
- Create: `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/icons/*`, `ui/index.html`, `.gitignore`, `LICENSE` (MIT)

**Interfaces:**
- Produces: крейт `claude_usage_monitor` (lib) + bin; `pub fn run()` в `lib.rs`; модули объявляются в `lib.rs` по мере появления (`pub mod model;` и т.д.).

- [ ] **Step 1: Установить инструменты (Windows)**

```bash
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id Rustlang.Rustup
cargo install tauri-cli --version "^2" --locked
```
Проверка: `rustc --version`, `cargo tauri --version` печатают версии. (WebView2 уже установлен.)

- [ ] **Step 2: Создать каркас**

`Cargo.toml`: `[lib] name = "claude_usage_monitor", crate-type = ["staticlib","cdylib","rlib"]`; зависимости `tauri = { version = "2", features = ["tray-icon", "image-png"] }`, `tauri-plugin-notification = "2"`, `tauri-plugin-autostart = "2"`, `tauri-plugin-single-instance = "2"`, `tauri-plugin-positioner = { version = "2", features = ["tray-icon"] }`, `reqwest = { version = "0.12", default-features = false, features = ["rustls-tls-native-roots"] }`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`, `time = { version = "0.3", features = ["serde", "parsing", "formatting", "macros"] }`, `sys-locale = "0.3"`, `tokio = { version = "1", features = ["sync", "time"] }`; `[build-dependencies] tauri-build = "2"`; release-профиль из Global Constraints.

`tauri.conf.json`: `productName: "Claude Usage Monitor"`, `identifier: "dev.claude-usage-monitor"`, `build.frontendDist: "../ui"`, `app.windows: []`, `bundle.targets: "all"`.

`lib.rs::run()`: строит `tauri::Builder` с плагином single-instance (пока пустой колбэк), создаёт трей с иконкой по умолчанию и меню из одного пункта «Quit»; в `RunEvent::ExitRequested { code: None, api, .. }` вызывает `api.prevent_exit()` (приложение живёт без окон). `main.rs`: `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`, вызывает `claude_usage_monitor::run()`.

Иконки: `cargo tauri icon` из любой 1024×1024 PNG (временно — простой квадрат с двумя полосами).

- [ ] **Step 3: Проверить запуск**

Run: `cargo tauri dev`
Expected: в трее появляется иконка, меню «Quit» завершает процесс, окон нет.

- [ ] **Step 4: Commit** — `chore: scaffold Tauri app`

---

### Task 1: Модель данных

**Files:**
- Create: `src-tauri/src/model.rs`

**Interfaces:**
- Produces:
```rust
pub enum Period { FiveHour, SevenDay, Unknown }       // Clone, Copy, PartialEq, Debug, Serialize
impl Period { pub fn from_key(key: &str) -> Period; pub fn duration(self) -> Option<time::Duration>; }
pub struct Quota { pub key: String, pub utilization: f64, pub resets_at: Option<OffsetDateTime>, pub period: Period }
pub struct Profile { pub email: Option<String>, pub plan: Option<String> }
pub struct Snapshot { pub quotas: Vec<Quota>, pub fetched_at: OffsetDateTime }
pub enum ApiError { NoToken, AuthExpired, RateLimited { retry_after: Option<u64> }, Server(u16), Network(String), Http(u16), Malformed }
pub struct AppState { pub snapshot: Option<Snapshot>, pub profile: Option<Profile>, pub last_error: Option<ApiError>, pub last_change: Option<OffsetDateTime> }
```
Все типы: `Clone, Debug, PartialEq`; `AppState: Default`.

- [ ] **Step 1: Тесты**

```rust
#[test] fn period_from_key() {
    assert_eq!(Period::from_key("five_hour"), Period::FiveHour);
    assert_eq!(Period::from_key("seven_day_opus"), Period::SevenDay);
    assert_eq!(Period::from_key("monthly"), Period::Unknown);
    assert_eq!(Period::FiveHour.duration(), Some(Duration::hours(5)));
    assert_eq!(Period::SevenDay.duration(), Some(Duration::days(7)));
    assert_eq!(Period::Unknown.duration(), None);
}
```
- [ ] **Step 2:** `cargo test model` → FAIL (не компилируется).
- [ ] **Step 3:** Реализовать типы; `from_key` — по префиксу `five_hour` / `seven_day`.
- [ ] **Step 4:** `cargo test model` → PASS.
- [ ] **Step 5: Commit** — `feat: data model`

---

### Task 2: Темп (`pace`)

**Files:**
- Create: `src-tauri/src/pace.rs`

**Interfaces:**
- Consumes: `Quota`, `Period` (Task 1)
- Produces: `pub fn elapsed_fraction(q: &Quota, now: OffsetDateTime) -> Option<f64>`; `pub fn is_ahead(q: &Quota, now: OffsetDateTime) -> bool`

`elapsed_fraction`: `None`, если нет `resets_at` или `period.duration()`; иначе `start = resets_at - d`, результат `((now - start) / d).clamp(0.0, 1.0)`. `is_ahead`: `utilization / 100 > elapsed_fraction` (false, если фракции нет).

- [ ] **Step 1: Тесты**

```rust
#[test] fn fraction_midway() {   // 5h-квота, сброс через 2.5 ч
    let q = quota("five_hour", 10.0, Some(now + Duration::minutes(150)));
    assert!((elapsed_fraction(&q, now).unwrap() - 0.5).abs() < 1e-9);
}
#[test] fn fraction_clamped_after_reset() {
    let q = quota("five_hour", 10.0, Some(now - Duration::minutes(5)));
    assert_eq!(elapsed_fraction(&q, now), Some(1.0));
}
#[test] fn no_fraction_without_reset_or_period() {
    assert_eq!(elapsed_fraction(&quota("five_hour", 10.0, None), now), None);
    assert_eq!(elapsed_fraction(&quota("monthly", 10.0, Some(now + Duration::hours(1))), now), None);
}
#[test] fn ahead_of_pace() {
    let q = quota("five_hour", 60.0, Some(now + Duration::minutes(150)));   // 60% при 50% времени
    assert!(is_ahead(&q, now));
    assert!(!is_ahead(&quota("five_hour", 40.0, Some(now + Duration::minutes(150))), now));
}
```
(`quota(key, util, resets_at)` — тестовый хелпер, `now = datetime!(2026-11-04 12:00 UTC)`.)
- [ ] **Step 2:** `cargo test pace` → FAIL.
- [ ] **Step 3:** Реализовать.
- [ ] **Step 4:** `cargo test pace` → PASS.
- [ ] **Step 5: Commit** — `feat: pace calculation`

---

### Task 3: Токен (`credentials`)

**Files:**
- Create: `src-tauri/src/credentials.rs`, `src-tauri/tests/fixtures/credentials.json`

**Interfaces:**
- Produces:
```rust
pub enum CredError { NotFound, Malformed, Io(String) }
pub struct Credentials { pub access_token: String, pub expires_at_ms: Option<i64> }   // ручной Debug, скрывающий токен
pub trait Keychain { fn read(&self) -> Option<String>; }
pub struct SystemKeychain;   // macOS: /usr/bin/security find-generic-password -s "Claude Code-credentials" -w; иначе всегда None
pub fn parse_credentials_json(s: &str) -> Result<Credentials, CredError>;
pub fn config_dir() -> PathBuf;   // $CLAUDE_CONFIG_DIR, иначе ~/.claude
pub fn find_credentials(env_token: Option<String>, keychain: &dyn Keychain, config_dir: &Path) -> Result<Credentials, CredError>;
pub fn load() -> Result<Credentials, CredError>;   // find_credentials(env CLAUDE_CODE_OAUTH_TOKEN, &SystemKeychain, &config_dir())
```
Порядок: непустой env → `Credentials { access_token: env, expires_at_ms: None }`; затем keychain; затем `<config_dir>/.credentials.json`. Файла нет → `NotFound`; ошибка чтения → `Io`.

- [ ] **Step 1: Тесты**

```rust
#[test] fn parses_token_and_expiry() {
    let c = parse_credentials_json(r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat-x","expiresAt":1767225600000}}"#).unwrap();
    assert_eq!(c.access_token, "sk-ant-oat-x");
    assert_eq!(c.expires_at_ms, Some(1767225600000));
}
#[test] fn missing_or_empty_token_is_malformed() {
    assert!(matches!(parse_credentials_json(r#"{"claudeAiOauth":{}}"#), Err(CredError::Malformed)));
    assert!(matches!(parse_credentials_json(r#"{"claudeAiOauth":{"accessToken":""}}"#), Err(CredError::Malformed)));
    assert!(matches!(parse_credentials_json(r#"{"other":1}"#), Err(CredError::Malformed)));
}
#[test] fn truncated_json_is_malformed() {
    assert!(matches!(parse_credentials_json(r#"{"claudeAiOauth":{"accessTo"#), Err(CredError::Malformed)));
}
#[test] fn env_wins_over_keychain_and_file() { /* env=Some("e"), FakeKeychain(Some(json "k")), файл "f" → "e" */ }
#[test] fn keychain_wins_over_file() { /* env=None → "k" */ }
#[test] fn falls_back_to_file() { /* FakeKeychain(None), tempdir с .credentials.json → "f" */ }
#[test] fn nothing_found() { /* пустой tempdir → Err(NotFound) */ }
#[test] fn debug_hides_token() { assert!(!format!("{:?}", creds("secret")).contains("secret")); }
```
(tempdir — `std::env::temp_dir()` + уникальное имя, без доп. зависимостей.)
- [ ] **Step 2:** `cargo test credentials` → FAIL.
- [ ] **Step 3:** Реализовать (`SystemKeychain` — `#[cfg(target_os = "macos")]` через `std::process::Command`, stdout → trim).
- [ ] **Step 4:** `cargo test credentials` → PASS.
- [ ] **Step 5: Commit** — `feat: credentials lookup`

---

### Task 4: API (`api`)

**Files:**
- Create: `src-tauri/src/api.rs`, `src-tauri/tests/fixtures/{usage_full.json, usage_extra.json, profile.json}`

**Interfaces:**
- Consumes: `Quota`, `Period`, `Profile`, `ApiError` (Task 1); `credentials::load` (Task 3)
- Produces:
```rust
pub const BASE_URL: &str = "https://api.anthropic.com";
pub fn parse_usage(body: &str) -> Result<Vec<Quota>, ApiError>;       // невалидный JSON → Malformed
pub fn parse_profile(body: &str) -> Option<Profile>;
pub fn map_status(status: u16, retry_after: Option<&str>) -> Option<ApiError>;   // 2xx → None
pub struct Client { /* reqwest::Client */ }
impl Client {
    pub fn new() -> Client;   // timeout 10 s, User-Agent claude-usage-monitor/<CARGO_PKG_VERSION>
    pub async fn fetch_usage(&self) -> Result<Vec<Quota>, ApiError>;    // credentials::load() → NoToken при NotFound/Malformed/Io
    pub async fn fetch_profile(&self) -> Option<Profile>;
}
```
`parse_usage`: каждое поле верхнего уровня — объект, содержащий ключи `utilization` и `resets_at`; при `utilization: null` пропускается; `resets_at` парсится как RFC 3339, null/непарсящееся → `None`. Порядок: `five_hour`, `seven_day`, затем остальные по алфавиту.
`parse_profile`: `email` из `account.email`; `plan`: `account.has_claude_max == true` → `"Max"`, иначе `account.has_claude_pro == true` → `"Pro"`, иначе `None`. **Фикстуру `profile.json` снять с реального ответа** (временный `#[ignore]`-тест или `curl` с токеном из `credentials.json`; токен в фикстуру и коммит не попадает — оставить только поля `account.email` (замаскированный), `has_claude_*`), и при расхождении структуры поправить разбор.
`map_status`: 401 → `AuthExpired`; 429 → `RateLimited { retry_after: retry_after.and_then(|v| v.trim().parse::<u64>().ok()) }`; 500–599 → `Server(code)`; прочие не-2xx → `Http(code)`. Ошибки reqwest (connect/timeout/TLS) → `Network(e.to_string())`.

- [ ] **Step 1: Тесты**

```rust
#[test] fn parses_full_usage() {   // usage_full.json: five_hour 42.0, seven_day 18.0, seven_day_opus 5.0
    let q = parse_usage(include_str!("../tests/fixtures/usage_full.json")).unwrap();
    assert_eq!(q.iter().map(|q| q.key.as_str()).collect::<Vec<_>>(), ["five_hour", "seven_day", "seven_day_opus"]);
    assert_eq!(q[0].utilization, 42.0);
    assert_eq!(q[0].period, Period::FiveHour);
    assert!(q[0].resets_at.is_some());
}
#[test] fn null_fields() {
    let q = parse_usage(r#"{"five_hour":{"utilization":10,"resets_at":null},"seven_day_sonnet":{"utilization":null,"resets_at":null}}"#).unwrap();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].resets_at, None);
}
#[test] fn ignores_non_quota_fields() {   // usage_extra.json: extra_usage {is_enabled, monthly_limit, ...}, "x": {}, "y": [1], "z": 5 + одна квота
    assert_eq!(parse_usage(include_str!("../tests/fixtures/usage_extra.json")).unwrap().len(), 1);
}
#[test] fn unknown_quota_key_kept() {
    let q = parse_usage(r#"{"seven_day_fable":{"utilization":3,"resets_at":"2026-11-10T00:00:00Z"}}"#).unwrap();
    assert_eq!(q[0].key, "seven_day_fable");
}
#[test] fn invalid_json_is_malformed() { assert_eq!(parse_usage("<html>"), Err(ApiError::Malformed)); }
#[test] fn status_mapping() {
    assert_eq!(map_status(200, None), None);
    assert_eq!(map_status(401, None), Some(ApiError::AuthExpired));
    assert_eq!(map_status(429, Some("120")), Some(ApiError::RateLimited { retry_after: Some(120) }));
    assert_eq!(map_status(503, None), Some(ApiError::Server(503)));
    assert_eq!(map_status(404, None), Some(ApiError::Http(404)));
}
#[test] fn retry_after_garbage_is_none() {
    assert_eq!(map_status(429, Some("Wed, 21 Oct 2026 07:28:00 GMT")), Some(ApiError::RateLimited { retry_after: None }));
    assert_eq!(map_status(429, Some("-5")), Some(ApiError::RateLimited { retry_after: None }));
}
#[test] fn parses_profile() { /* profile.json → email Some(..), plan Some("Max"|"Pro") по фикстуре */ }
```
- [ ] **Step 2:** `cargo test api` → FAIL.
- [ ] **Step 3:** Реализовать разбор, `map_status`, `Client`.
- [ ] **Step 4:** `cargo test api` → PASS. Дополнительно один раз вручную: `#[ignore] async fn live_usage()` печатает число квот; `cargo test api::live -- --ignored --nocapture` → печатает ≥ 2 квоты.
- [ ] **Step 5: Commit** — `feat: usage API client`

---

### Task 5: Настройки и локализация (`settings`, `i18n`)

**Files:**
- Create: `src-tauri/src/settings.rs`, `src-tauri/src/i18n.rs`, `src-tauri/locales/ru.json`, `src-tauri/locales/en.json`

**Interfaces:**
- Produces:
```rust
// settings.rs
#[derive(Deserialize, Clone, Debug, PartialEq)] #[serde(default, deny_unknown_fields)]
pub struct Settings { pub language: String, pub thresholds: Vec<u8>, pub notify_reset: bool, pub pace_alerts: bool,
    pub poll_active_secs: u64, pub poll_idle_secs: u64, pub hidden_quotas: Vec<String>, pub tray_bars: Vec<String> }
// Default: "auto", [80,95], true, false, 180, 600, [], ["five_hour","seven_day"]
pub const FILE_NAME: &str = "usage-monitor-settings.json";
pub fn search_dirs() -> Vec<PathBuf>;   // [$CLAUDE_CONFIG_DIR?, папка exe, ~/.claude]
pub fn load_from(dirs: &[PathBuf]) -> (Settings, Option<String>);   // первый найденный файл; ошибка → (Default, Some(текст ошибки))

// i18n.rs
pub enum Lang { Ru, En }
pub fn resolve_lang(setting: &str, system: Option<&str>) -> Lang;   // "ru"/"en" явно; "auto" → system начинается с "ru" → Ru, иначе En
pub struct Strings { /* lang + HashMap<String,String> из include_str! */ }
impl Strings { pub fn new(lang: Lang) -> Strings; pub fn t(&self, key: &str) -> &str; pub fn lang(&self) -> Lang;
    pub fn all(&self) -> &HashMap<String, String>; }   // отсутствующий ключ → сам ключ
pub fn format_duration(d: time::Duration, lang: Lang) -> String;
pub fn quota_name(key: &str, lang: Lang) -> String;      // полное имя для попапа/уведомлений
pub fn quota_short(key: &str, lang: Lang) -> String;     // для подсказки
```
`format_duration`: `d <= 0` → `"скоро"`/`"soon"`; < 1 ч → `"14 мин"`/`"14m"`; < 24 ч → `"2 ч 14 мин"`/`"2h 14m"`; иначе `"3 д 4 ч"`/`"3d 4h"`.
`quota_name`: `five_hour` → `"5 часов"`/`"5 hours"`; `seven_day` → `"7 дней"`/`"7 days"`; `seven_day_<x>` → `"7 дней · X"`/`"7 days · X"` (первая буква `x` заглавная, `_` → пробел); иначе ключ с `_` → пробел и заглавной буквой.
`quota_short`: `five_hour` → `"5ч"`/`"5h"`; `seven_day` → `"7д"`/`"7d"`; иначе `quota_name`.
Ключи локалей (одинаковые в обоих файлах): `menu_open, menu_refresh, menu_autostart, menu_quit, tooltip_updated_ago, err_no_token, err_auth_expired, err_rate_limited, err_network, err_server, err_http, err_settings, popup_updated, popup_refresh, popup_stale, popup_resets_in, alert_threshold, alert_reset, app_name`. Плейсхолдеры: `{name}`, `{percent}`, `{time}`, `{code}`, `{msg}`.

- [ ] **Step 1: Тесты**

```rust
#[test] fn defaults_when_no_file() { assert_eq!(load_from(&[empty_dir]), (Settings::default(), None)); }
#[test] fn partial_file_overrides_only_given_keys() { /* {"thresholds":[50]} → thresholds [50], остальное default */ }
#[test] fn broken_file_gives_default_and_error() { /* "{oops" → (default, Some(_)) */ }
#[test] fn unknown_key_is_error() { /* {"treshold":[1]} → (default, Some(_)) */ }
#[test] fn first_dir_wins() { /* два каталога с разными файлами → значения из первого */ }
#[test] fn lang_resolution() {
    assert_eq!(resolve_lang("auto", Some("ru-RU")), Lang::Ru);
    assert_eq!(resolve_lang("auto", Some("de-DE")), Lang::En);
    assert_eq!(resolve_lang("auto", None), Lang::En);
    assert_eq!(resolve_lang("ru", Some("en-US")), Lang::Ru);
}
#[test] fn format_durations() {
    assert_eq!(format_duration(Duration::minutes(14), Lang::Ru), "14 мин");
    assert_eq!(format_duration(Duration::minutes(134), Lang::En), "2h 14m");
    assert_eq!(format_duration(Duration::hours(76), Lang::Ru), "3 д 4 ч");
}
#[test] fn format_negative_is_soon() {
    assert_eq!(format_duration(Duration::minutes(-3), Lang::Ru), "скоро");
    assert_eq!(format_duration(Duration::ZERO, Lang::En), "soon");
}
#[test] fn quota_names() {
    assert_eq!(quota_name("seven_day_opus", Lang::Ru), "7 дней · Opus");
    assert_eq!(quota_name("monthly_cap", Lang::En), "Monthly cap");
    assert_eq!(quota_short("five_hour", Lang::Ru), "5ч");
}
#[test] fn locales_have_same_keys() { /* множества ключей ru.json и en.json равны, все ключи из списка выше присутствуют */ }
```
- [ ] **Step 2:** `cargo test settings i18n` → FAIL.
- [ ] **Step 3:** Реализовать + заполнить `ru.json`/`en.json`.
- [ ] **Step 4:** `cargo test settings i18n` → PASS.
- [ ] **Step 5: Commit** — `feat: settings and i18n`

---

### Task 6: Уведомления — логика (`alerts`)

**Files:**
- Create: `src-tauri/src/alerts.rs`

**Interfaces:**
- Consumes: `Snapshot`, `Quota` (Task 1); `pace::is_ahead` (Task 2); `Settings` (Task 5)
- Produces:
```rust
pub enum Alert { Threshold { key: String, threshold: u8, utilization: f64, resets_at: Option<OffsetDateTime> }, Reset { key: String } }
#[derive(Default)] pub struct FiredSet(HashSet<(String, u8, Option<i64>)>);   // (key, порог, resets_at unix)
pub fn evaluate(prev: Option<&Snapshot>, next: &Snapshot, fired: &mut FiredSet, s: &Settings, now: OffsetDateTime) -> Vec<Alert>;
```
Правила: квоты из `s.hidden_quotas` игнорируются. Для каждой квоты и порога `t`, если `utilization >= t` и `(key,t,resets_at)` нет в `fired` — добавить в `fired`; Alert выдаётся, только если `prev.is_some()` и (не `pace_alerts` или `is_ahead`). Reset: если `notify_reset`, в `prev` есть квота с тем же ключом, `resets_at` отличается и `prev.utilization >= 80.0` → `Alert::Reset`.

- [ ] **Step 1: Тесты**

```rust
#[test] fn first_snapshot_is_silent_but_marks_fired() {
    let mut f = FiredSet::default();
    assert!(evaluate(None, &snap(&[("five_hour", 85.0, R1)]), &mut f, &s, now).is_empty());
    assert!(evaluate(Some(&snap(&[("five_hour", 85.0, R1)])), &snap(&[("five_hour", 86.0, R1)]), &mut f, &s, now).is_empty());
}
#[test] fn crossing_threshold_fires_once() {
    // 70 → 82: один Threshold{80}; 82 → 84: ничего; 84 → 96: один Threshold{95}
}
#[test] fn jump_over_two_thresholds_fires_both() { /* 70 → 97 → два алерта: 80 и 95 */ }
#[test] fn rearms_after_reset() { /* 85 (R1) → 10 (R2) → 85 (R2): снова Threshold{80} */ }
#[test] fn reset_alert_only_if_was_high() {
    // 90 (R1) → 2 (R2) → [Reset]; 50 (R1) → 2 (R2) → []
}
#[test] fn pace_mode_suppresses_when_not_ahead() { /* pace_alerts=true, 5h-квота 82% при 90% прошедшего времени → [] */ }
#[test] fn hidden_quota_ignored() { /* hidden_quotas=["five_hour"] → [] */ }
```
- [ ] **Step 2:** `cargo test alerts` → FAIL.
- [ ] **Step 3:** Реализовать.
- [ ] **Step 4:** `cargo test alerts` → PASS.
- [ ] **Step 5: Commit** — `feat: alert evaluation`

---

### Task 7: Расписание опроса (`poller` — чистая часть)

**Files:**
- Create: `src-tauri/src/poller.rs`

**Interfaces:**
- Consumes: `ApiError`, `Snapshot` (Task 1); `Settings` (Task 5)
- Produces:
```rust
pub struct PollCtx { pub now: OffsetDateTime, pub last_change: Option<OffsetDateTime>, pub next_reset: Option<OffsetDateTime>,
                     pub popup_open: bool, pub backoff: Option<time::Duration> }
pub fn next_delay(ctx: &PollCtx, s: &Settings) -> time::Duration;
pub fn next_backoff(prev: Option<time::Duration>, err: &ApiError) -> Option<time::Duration>;
pub fn next_reset(snap: &Snapshot) -> Option<OffsetDateTime>;            // ближайший будущий resets_at
pub fn utilization_changed(prev: Option<&Snapshot>, next: &Snapshot) -> bool;
```
`next_delay`: если `backoff` → он; иначе база = 60 с при `popup_open`, `poll_active_secs` если `last_change` не старше 15 мин, иначе `poll_idle_secs`; если `next_reset + 30 с - now` меньше базы → это значение; результат не меньше 30 с.
`next_backoff`: `AuthExpired` → 60 с; `RateLimited{Some(n)}` → `n` с (не меньше 30, не больше 900); `RateLimited{None}`, `Server`, `Network` → `prev*2` (старт 60, максимум 900); `NoToken`, `Http`, `Malformed` → `None`.

- [ ] **Step 1: Тесты**

```rust
#[test] fn idle_and_active_intervals() { /* last_change None → 600 с; last_change now-5мин → 180 с; now-20мин → 600 с */ }
#[test] fn popup_open_is_one_minute() { /* → 60 с */ }
#[test] fn aligns_to_reset() { /* next_reset = now+2мин, idle → 150 с */ }
#[test] fn never_below_30s() { /* next_reset = now-1мин → 30 с; backoff 5 с → 30 с */ }
#[test] fn backoff_doubles_and_caps() {
    let e = ApiError::Network("x".into());
    assert_eq!(next_backoff(None, &e), Some(Duration::seconds(60)));
    assert_eq!(next_backoff(Some(Duration::seconds(60)), &e), Some(Duration::seconds(120)));
    assert_eq!(next_backoff(Some(Duration::seconds(600)), &e), Some(Duration::seconds(900)));
    assert_eq!(next_backoff(None, &ApiError::RateLimited { retry_after: None }), Some(Duration::seconds(60)));
}
#[test] fn retry_after_respected() { assert_eq!(next_backoff(None, &ApiError::RateLimited { retry_after: Some(120) }), Some(Duration::seconds(120))); }
#[test] fn auth_expired_retries_in_a_minute() { assert_eq!(next_backoff(Some(Duration::seconds(900)), &ApiError::AuthExpired), Some(Duration::seconds(60))); }
#[test] fn no_backoff_for_http() { assert_eq!(next_backoff(None, &ApiError::Http(404)), None); }
#[test] fn change_detection() { /* None→snap: true; одинаковые: false; 42→43: true */ }
```
- [ ] **Step 2:** `cargo test poller` → FAIL.
- [ ] **Step 3:** Реализовать.
- [ ] **Step 4:** `cargo test poller` → PASS.
- [ ] **Step 5: Commit** — `feat: poll scheduling`

---

### Task 8: Иконка трея (`tray_icon`)

**Files:**
- Create: `src-tauri/src/tray_icon.rs`

**Interfaces:**
- Produces:
```rust
pub enum BarColor { Blue, Orange, Red }
pub fn bar_color(utilization: f64, ahead: bool) -> BarColor;      // ≥95 или ahead → Red; ≥80 → Orange; иначе Blue
pub struct Bar { pub utilization: f64, pub color: BarColor }
pub fn render(bars: &[Option<Bar>], error: bool, size: u32, dark_panel: bool) -> Vec<u8>;   // RGBA, len = size*size*4
pub fn panel_is_dark() -> bool;   // Windows: HKCU\...\Themes\Personalize\SystemUsesLightTheme == 0; иначе true
```
Геометрия (для `size`): две горизонтальные полосы высотой `size*0.35`, отступ `size*0.1`, по ширине вся иконка с отступом 1 px; фон полосы — полупрозрачный (белый на тёмной панели, чёрный на светлой), заливка слева на `utilization.clamp(0,100)/100` ширины. `None` в `bars` → пустая полоса. `error` → вместо полос красный круг с белым «!» (вертикальный прямоугольник + точка). Цвета: Blue `#4A90E2`, Orange `#F5A623`, Red `#E5484D`.

- [ ] **Step 1: Тесты**

```rust
#[test] fn buffer_size() { assert_eq!(render(&[None, None], false, 32, true).len(), 32 * 32 * 4); }
#[test] fn colors() {
    assert_eq!(bar_color(50.0, false), BarColor::Blue);
    assert_eq!(bar_color(80.0, false), BarColor::Orange);
    assert_eq!(bar_color(95.0, false), BarColor::Red);
    assert_eq!(bar_color(10.0, true), BarColor::Red);
}
#[test] fn fill_proportional() {   // верхняя полоса 50%: пиксель на 25% ширины — синий, на 75% — не синий
    let px = render(&[Some(Bar { utilization: 50.0, color: BarColor::Blue }), None], false, 32, true);
    assert_eq!(rgb_at(&px, 32, 8, top_bar_y), BLUE);
    assert_ne!(rgb_at(&px, 32, 24, top_bar_y), BLUE);
}
#[test] fn over_100_clamped() { /* utilization 150 → пиксель у правого края полосы синий, нет паники */ }
#[test] fn error_icon_differs() { assert_ne!(render(&[None, None], true, 32, true), render(&[None, None], false, 32, true)); }
```
- [ ] **Step 2:** `cargo test tray_icon` → FAIL.
- [ ] **Step 3:** Реализовать (ручная заливка прямоугольников в `Vec<u8>`).
- [ ] **Step 4:** `cargo test tray_icon` → PASS.
- [ ] **Step 5: Commit** — `feat: tray icon renderer`

---

### Task 9: Представление для UI (`view`)

**Files:**
- Create: `src-tauri/src/view.rs`

**Interfaces:**
- Consumes: `AppState`, `ApiError` (Task 1); `pace` (Task 2); `Strings`, `format_duration`, `quota_name`, `quota_short` (Task 5); `Settings` (Task 5); `bar_color`, `BarColor` (Task 8)
- Produces:
```rust
#[derive(Serialize)] pub struct QuotaView { pub key: String, pub name: String, pub percent: u8, pub elapsed: Option<f64>,
    pub color: &'static str /* "blue"|"orange"|"red" */, pub reset_text: Option<String> }
#[derive(Serialize)] pub struct PopupView { pub email: Option<String>, pub plan: Option<String>, pub quotas: Vec<QuotaView>,
    pub updated_text: Option<String>, pub stale: bool, pub error_text: Option<String> }
pub fn build_view(st: &AppState, s: &Settings, strings: &Strings, now: OffsetDateTime) -> PopupView;
pub fn tooltip_text(st: &AppState, s: &Settings, strings: &Strings, now: OffsetDateTime) -> String;
pub fn error_text(e: &ApiError, strings: &Strings) -> String;
pub fn tray_bars(st: &AppState, s: &Settings, now: OffsetDateTime) -> Vec<Option<Bar>>;   // по s.tray_bars
```
`stale` = `now - fetched_at > 2 * poll_idle_secs`. `percent` = `utilization.round().clamp(0,100)`. `reset_text` = `popup_resets_in` с `{time}` = `format_duration(resets_at - now)`. `hidden_quotas` исключаются. `tooltip_text`: при `last_error` и отсутствии снапшота — только текст ошибки; иначе `"5ч: 42% · 7д: 18%"` по `tray_bars`, + `"\n" + error_text` если есть ошибка, + `tooltip_updated_ago` если `stale`; первая строка с префиксом `app_name`. Ошибки: `NoToken` → `err_no_token` (содержит `claude auth login`), `AuthExpired` → `err_auth_expired`, и т.д.

- [ ] **Step 1: Тесты**

```rust
#[test] fn view_basic() { /* five_hour 42 (сброс через 134 мин), Ru → percent 42, reset_text "сброс через 2 ч 14 мин", color "blue" */ }
#[test] fn percent_is_clamped() { /* utilization 130.0 → 100; -2.0 → 0 */ }
#[test] fn no_reset_no_marker() { /* resets_at None → reset_text None, elapsed None */ }
#[test] fn error_keeps_last_snapshot() { /* snapshot есть + last_error Network → quotas не пусты, error_text Some */ }
#[test] fn stale_flag() { /* fetched_at = now - 21 мин при idle 600 → stale true; now - 5 мин → false */ }
#[test] fn tooltip_format() { /* Ru, 42 и 18 → содержит "5ч: 42% · 7д: 18%" */ }
#[test] fn tooltip_no_token() { /* AppState{last_error: NoToken} → содержит "claude auth login" */ }
#[test] fn tray_bars_follow_settings() { /* tray_bars ["seven_day","missing"] → [Some(18%), None] */ }
```
- [ ] **Step 2:** `cargo test view` → FAIL.
- [ ] **Step 3:** Реализовать.
- [ ] **Step 4:** `cargo test view` → PASS; `cargo test` целиком → PASS.
- [ ] **Step 5: Commit** — `feat: view models for tray and popup`

---

### Task 10: Сборка приложения — трей, опрос, уведомления, автозапуск

**Files:**
- Create: `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/src/poller.rs` (добавить цикл), `src-tauri/capabilities/default.json`

**Interfaces:**
- Consumes: всё из Tasks 1–9
- Produces:
```rust
// lib.rs
pub struct Shared { pub state: Mutex<AppState>, pub settings: Settings, pub strings: Strings, pub fired: Mutex<FiredSet>,
                    pub refresh: tokio::sync::Notify, pub popup_open: AtomicBool }
// poller.rs
pub fn spawn(app: tauri::AppHandle);          // цикл в tauri::async_runtime
pub fn request_refresh(app: &tauri::AppHandle);   // троттлинг 10 с, будит цикл через Notify
// tray.rs
pub const TRAY_ID: &str = "main";
pub fn create(app: &tauri::AppHandle) -> tauri::Result<()>;
pub fn update(app: &tauri::AppHandle);        // иконка (render 32px, на macOS 44px) + tooltip из view
```
Цикл `spawn`: `api::Client::fetch_usage` → при успехе новый `Snapshot`, обновить `last_change` если `utilization_changed`, сбросить backoff, `alerts::evaluate` → отправить уведомления через `tauri-plugin-notification` (тексты `alert_threshold`/`alert_reset`); при ошибке — `last_error`, снапшот не трогать, `next_backoff`. Профиль — при старте и раз в час. Затем `tray::update`, `app.emit("state", build_view(..))`. Ожидание: `tokio::select!` между `sleep(next_delay)` и `refresh.notified()`. Пробуждение после сна: если реальное время ожидания > 2× запланированного → следующий запрос сразу.
Меню трея: `menu_open`, `menu_refresh`, чекбокс `menu_autostart` (состояние из `autolaunch().is_enabled()`), `menu_quit`. Левый клик (Windows) → `popup::toggle` (Task 11; до этого — заглушка). macOS: `show_menu_on_left_click(true)`. Linux: левый клик недоступен — попап только из меню.
Single-instance колбэк → `popup::show`. Ошибка в настройках → одно уведомление `err_settings` при старте. Linux без трея: ошибка `TrayIconBuilder::build` → `eprintln!` + уведомление + `app.exit(1)`.

- [ ] **Step 1:** Реализовать `tray.rs`, цикл в `poller.rs`, подключение плагинов и `Shared` в `lib.rs`; добавить разрешения `notification:default`, `autostart:default` в capabilities.
- [ ] **Step 2:** `cargo clippy -- -D warnings` и `cargo test` → без ошибок.
- [ ] **Step 3: Ручная проверка** — `cargo tauri dev`: иконка показывает 2 полосы с реальными процентами; подсказка «5ч: N% · 7д: M%»; «Обновить» обновляет; чекбокс автозапуска меняет запись в `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; переименование `~/.claude/.credentials.json` (временно, с возвратом) → иконка «!» и подсказка с `claude auth login` в течение 1–10 мин или сразу после «Обновить».
- [ ] **Step 4: Commit** — `feat: wire tray, polling, notifications, autostart`

---

### Task 11: Попап

**Files:**
- Create: `src-tauri/src/popup.rs`, `ui/index.html`, `ui/popup.css`, `ui/popup.js`
- Modify: `src-tauri/src/lib.rs` (регистрация команд), `src-tauri/src/tray.rs` (клик → toggle), `src-tauri/capabilities/default.json`

**Interfaces:**
- Consumes: `Shared` (Task 10), `build_view` (Task 9), `poller::request_refresh` (Task 10)
- Produces:
```rust
pub const LABEL: &str = "popup";
pub fn show(app: &AppHandle); pub fn toggle(app: &AppHandle);
#[tauri::command] fn get_state(app: AppHandle) -> PopupView;
#[tauri::command] fn get_strings(app: AppHandle) -> HashMap<String, String>;
#[tauri::command] fn refresh(app: AppHandle);
#[tauri::command] fn close_popup(app: AppHandle);
```
Окно: 320×(по контенту, max 480) px, `decorations(false)`, `always_on_top(true)`, `skip_taskbar(true)`, `resizable(false)`, позиция `Position::TrayCenter` (positioner; на Linux — `TopRight`). `WindowEvent::Focused(false)` и `close_popup` → `window.destroy()` + `popup_open = false`; при создании `popup_open = true` и `request_refresh`. Esc в JS → `close_popup`.
`popup.js`: при загрузке `get_strings` + `get_state`, подписка `listen("state")`; рендер: шапка (email · plan), для каждой квоты — имя, `percent%`, полоса (`width: percent%`, класс по `color`), белая вертикальная линия на `elapsed*100%` (если не null), `reset_text`; плашка `popup_stale` при `stale`; блок ошибки при `error_text`; низ — `updated_text` + кнопка `popup_refresh`. Темы через `prefers-color-scheme`. `app.withGlobalTauri = true` в конфиге (без бандлера).

- [ ] **Step 1:** Реализовать `popup.rs` и `ui/*`.
- [ ] **Step 2:** `cargo clippy -- -D warnings`, `cargo test` → без ошибок.
- [ ] **Step 3: Ручная проверка** — `cargo tauri dev`: клик по иконке открывает попап у трея; проценты совпадают с claude.ai → Settings → Usage; Esc и клик вне окна закрывают; повторный клик снова открывает; «Обновить» обновляет время; тёмная тема Windows → тёмный попап; Диспетчер задач: после закрытия попапа процессы `msedgewebview2` приложения исчезают, память процесса ≤ 30 МБ.
- [ ] **Step 4: Commit** — `feat: popup window`

---

### Task 12: CI, выпуск и документация

**Files:**
- Create: `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `README.md`, `PRIVACY.md`, `docs/manual-test.md`

- [ ] **Step 1: `ci.yml`** — на push/PR, матрица `windows-latest`, `macos-latest`, `ubuntu-22.04`; на Ubuntu `apt-get install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`; шаги в `src-tauri`: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`.
- [ ] **Step 2: `release.yml`** — по тегу `v*`, `tauri-apps/tauri-action@v0` (projectPath `.`, `tauriScript: cargo tauri`), матрица: windows-latest; macos-latest с `--target aarch64-apple-darwin` и `--target x86_64-apple-darwin`; ubuntu-22.04. Создаёт черновик GitHub Release с `.msi`, `.exe`, `.dmg`, `.deb`, `.AppImage`.
- [ ] **Step 3: Документы** — `README.md` (RU + краткий EN: что делает, скриншот, установка на 3 ОС, требование «залогинен в Claude Code», пример `usage-monitor-settings.json` со всеми ключами и значениями по умолчанию из Task 5, «не связано с Anthropic»); `PRIVACY.md` (единственный хост, токен не хранится, что пишется в систему: только запись автозапуска); `docs/manual-test.md` — чек-лист из шагов ручной проверки Tasks 10–11 + macOS (меню по клику, токен из Связки ключей) + Linux (попап из меню).
- [ ] **Step 4: Проверка размера** — `cargo tauri build` локально; `src-tauri/target/release/claude-usage-monitor.exe` и `.msi` ≤ 15 МБ.
- [ ] **Step 5: Commit** — `chore: CI, release workflow and docs`
