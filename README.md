# Claude Usage Tray

Лёгкое приложение для системного трея, которое показывает лимиты подписки Claude в реальном времени:
5-часовую сессию, недельный лимит и модельные квоты. Работает на Windows, macOS и Linux.

*A lightweight tray app showing your Claude subscription limits (5-hour session, weekly and per-model
quotas) in real time. Windows, macOS, Linux. English summary below.*

> Это неофициальный проект сообщества, он не создан, не одобрен и не поддерживается Anthropic.

## Что умеет

- Иконка в трее с двумя полосами: 5 часов и 7 дней. Синий — нормально, оранжевый — от 80 %,
  красный — от 95 % или если расход обгоняет время.
- Подсказка при наведении: `5ч: 42% · 7д: 18%`.
- Попап по клику (на Linux — пункт меню «Открыть»): все квоты, таймеры до сброса и белая линия
  «сколько периода уже прошло».
- Уведомления при 80 % и 95 % и при восстановлении лимита.
- Автозапуск при входе (галочка в меню трея).
- Русский и английский интерфейс, язык выбирается по системе.
- Ничего не нужно настраивать: используется вход, который уже сделан в Claude Code.

## Требования

- Подписка Claude (Pro или Max) и вход в Claude Code (`claude auth login`).
  Оплата по API через Anthropic Console не поддерживается: у неё нет таких лимитов.
- Windows 10/11 (WebView2 уже есть в системе), macOS 11+, или Linux с поддержкой
  AppIndicator (GNOME с расширением AppIndicator, KDE Plasma и др.).

## Установка

Скачайте установщик со страницы [Releases](../../releases):

| ОС | Файл |
|---|---|
| Windows | `Claude Usage Tray_x.y.z_x64-setup.exe` или `.msi` |
| macOS (Apple Silicon / Intel) | `Claude Usage Tray_x.y.z_aarch64.dmg` / `_x64.dmg` |
| Linux | `.deb` или `.AppImage` |

Сборки пока не подписаны. Поэтому Windows SmartScreen предложит «Подробнее → Выполнить в любом
случае», а на macOS при первом запуске нужно открыть приложение через правый клик → «Открыть».

### Сборка из исходников

Нужны [Rust](https://rustup.rs) и Tauri CLI (`cargo install tauri-cli --version "^2" --locked`).
На Windows дополнительно Visual Studio Build Tools с компонентом C++, на Linux —
`libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`.

```bash
cd src-tauri
cargo tauri build      # установщики появятся в src-tauri/target/release/bundle/
cargo test --lib       # юнит-тесты
```

## Настройки (необязательно)

Положите файл `usage-monitor-settings.json` в `~/.claude/` (или в `$CLAUDE_CONFIG_DIR`, или рядом с
исполняемым файлом) и укажите только то, что хотите поменять. Значения по умолчанию:

```json
{
  "language": "auto",
  "thresholds": [80, 95],
  "notify_reset": true,
  "pace_alerts": false,
  "poll_active_secs": 180,
  "poll_idle_secs": 600,
  "hidden_quotas": [],
  "tray_bars": ["five_hour", "seven_day"]
}
```

- `language`: `auto`, `ru` или `en`.
- `pace_alerts`: предупреждать о порогах только тогда, когда расход обгоняет прошедшее время.
- `hidden_quotas`: ключи квот, которые не нужно показывать (например, `"seven_day_opus"`).
- `tray_bars`: какие две квоты рисовать в иконке.

Приложение никогда не создаёт и не изменяет этот файл.

## Откуда данные

Токен берётся из входа Claude Code: переменная `CLAUDE_CODE_OAUTH_TOKEN`, затем Связка ключей macOS
(«Claude Code-credentials»), затем файл `~/.claude/.credentials.json`. Приложение обращается только
к `api.anthropic.com`. Подробности — в [PRIVACY.md](PRIVACY.md).

## English

Tray app for Windows/macOS/Linux showing Claude subscription usage: icon with two bars (5h and
7-day), tooltip, popup with every quota and reset countdowns, threshold and reset notifications,
start at login, Russian/English UI. Requires being signed in to Claude Code. Download installers from
Releases; unsigned builds need the SmartScreen / Gatekeeper override on first run. Optional settings
file and keys are described above. Not affiliated with Anthropic.

## Лицензия

MIT — см. [LICENSE](LICENSE).
