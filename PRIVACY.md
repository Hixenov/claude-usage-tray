# Конфиденциальность

- **Сеть.** Приложение отправляет запросы только на `https://api.anthropic.com`
  (`/api/oauth/usage` и `/api/oauth/profile`). Других серверов, аналитики и телеметрии нет.
- **Токен.** OAuth-токен Claude Code читается перед каждым запросом из переменной
  `CLAUDE_CODE_OAUTH_TOKEN`, из Связки ключей macOS или из `~/.claude/.credentials.json`.
  Он используется только в заголовке `Authorization`, нигде не сохраняется и не пишется в логи.
  Приложение не обновляет токен само — это делает Claude Code.
- **Данные об использовании** хранятся только в памяти и пропадают при выходе.
- **Что записывается в систему:** только запись автозапуска, и только если вы включили
  «Запускать при входе» (Windows — `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
  macOS — LaunchAgent, Linux — `~/.config/autostart/`). Файл настроек приложение только читает.

---

**Privacy (English).** The app talks only to `api.anthropic.com`, reads the Claude Code OAuth token
before each request and never stores or logs it, keeps usage data in memory only, and writes nothing
to the system except the optional start-at-login entry.
