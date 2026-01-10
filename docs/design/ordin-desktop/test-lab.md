# ordin desktop — local test lab

**Status:** Draft, January 2026. **Covers:** the confirmation checks still open after decision 50 (C-48 to C-59 in 10-risks.md). **Scope set by the user, January 2026:** only what can be tested on this Mac now; everything else waits (section 8). **Runs on:** the user's MacBook (Apple Silicon, macOS 14.6.1, 16 GB memory).

Each test either emulates a system on the Mac, or uses a throwaway account with the real public service. A test that needs the company's own settings is marked **company only**: no emulator can answer it, and it goes to IT.

Results are recorded in 10-risks.md against the check they settle. Lab working files live outside the repository and are deleted when the lab is removed.

## 1. Lab parts

| Part | What it is | Removal |
|---|---|---|
| Linux virtual machine `ordin-lab` | Lima, template `debian-13` (systemd 257), 2 CPUs, 2 GiB memory, 20 GiB sparse disk | `limactl delete ordin-lab` |
| Distribution containers | Docker images used once to read each distribution's systemd package version | `docker image rm` for each image |
| NATS server | The installed `nats-server` v2.10.14, started on 127.0.0.1 with throwaway configurations | Stops with the test |
| Logging proxy | A Python HTTP CONNECT proxy on 127.0.0.1 that logs every connection | Stops with the test |
| Telegram test bot | A bot made in BotFather by the user, plus a private test group and channel | `/deletebot` in BotFather |
| Slack test workspace | A free workspace or a Slack developer sandbox, plus a test app, made by the user | Delete the workspace |
| Graph mock (SharePoint) | Microsoft Dev Proxy returning recorded Microsoft Graph responses (section 7) | Uninstall |

## 2. Linux server (C-50, C-51, C-57, C-59, OQ-44)

| # | Test | Pass rule |
|---|---|---|
| L1 | Read the systemd version each common server distribution ships (Debian 12 and 13, Ubuntu 22.04, 24.04 and 26.04, AlmaLinux and Rocky 9 and 10, Fedora 42) | A version is recorded for each |
| L2 | `systemd-creds encrypt --user` and `decrypt --user` as a normal user, then a user unit reading the secret through `LoadCredentialEncrypted=` | Works without root |
| L3 | `loginctl enable-linger` as a normal user | Works without root on a default install |
| L4 | User unit with `Type=notify`, `WatchdogSec=3`, `RestartSteps=3` and `RestartMaxDelaySec=20s`, whose main process stops sending watchdog messages | systemd stops it and restarts it with growing delays |
| L5 | `systemd-creds --help` on systemd 252 | `--user` is absent, which confirms older servers need the container image |
| L6 | Copilot CLI installed in the virtual machine; the user runs `copilot login --device-code` over `limactl shell` | Sign-in completes; record where the token is kept when no keyring is present |

## 3. NATS (C-53, C-54)

A test program pinned to `async-nats` 0.50.0 runs against the local server.

| # | Test | Pass rule |
|---|---|---|
| N1 | Durable pull consumer, explicit acknowledgement, 2-second acknowledgement wait | Messages that aren't acknowledged come back; the consumer survives a reconnect |
| N2 | Server stopped and restarted mid-run | The client reconnects with default options, and nothing is lost |
| N3 | Two publishes with the same `Nats-Msg-Id` | One message is stored |
| N4 | 2 MB message against the default 1 MB `max_payload`, then against 8 MB | Refused, then accepted |
| N5 | User and password, token, and a credentials file | Each one connects |
| N6 | `ws://` and `wss://` with a self-signed authority | Both connect |
| N7 | `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` set to the logging proxy | The proxy logs nothing, which confirms the client goes direct (decision 50) |

**Company only:** the company server's `max_payload` and whether ordin's machines can reach it directly.

## 4. Telegram (C-52, C-55, C-58)

The user creates the bot and puts its token in `~/.config/ordin-lab/telegram-token`, readable by them only. The lab script never prints the token or any message text. It records only update kinds, chat types and whether each message was a command, a reply or plain text.

| # | Test | Pass rule |
|---|---|---|
| T1 | `getMe` | `can_read_all_group_messages` is false by default |
| T2 | Bot in a private test group with privacy mode on; the user sends a plain message, a `/command`, and a reply to the bot | Only the command and the reply arrive |
| T3 | `/setprivacy` → Disable, then remove the bot from the group and add it again | All three arrive, and `can_read_all_group_messages` is true |
| T4 | Bot made admin of a private test channel; the user posts | A `channel_post` update arrives |
| T5 | Two `getUpdates` receivers at once | The second gets 409 (observed behaviour, decision 50) |
| T6 | A 4,097-character message and a 65-byte `callback_data` | Both refused, and the error text is recorded |
| T7 | `tg://resolve?domain=BotFather` opened from the Mac | Telegram opens the BotFather chat |

## 5. Slack (C-56)

The user makes a free workspace and a test app from a manifest. The lab supplies the manifest.

| # | Test | Pass rule |
|---|---|---|
| S1 | Socket Mode connection with an `xapp-` token that has `connections:write` | Connects; each envelope is acknowledged |
| S2 | A direct message to the app (`message.im`, `im:history`, Messages tab on) | The event arrives |
| S3 | Which messaging experience the new app is offered | Recorded, Agent or classic |
| S4 | File upload with `files.getUploadURLExternal` and `files.completeUploadExternal` | The file appears in the direct message |
| S5 | Message of 40,001 characters and one of 51 blocks | Both refused, and the error text is recorded |

**Company only:** whether the company workspace requires app approval.

## 6. Desktop

| # | Test | Where |
|---|---|---|
| D1 | macOS reports the `tg:` handler without reading Telegram's data | Done, January 2026 (C-59) |
| D2 | The same check on Windows | A colleague's Windows PC, for five minutes |
| D3 | Dictation through SpeechAnalyzer | A macOS 26 virtual machine; the result counts only if the microphone passes through |

## 7. SharePoint and Microsoft sign-in (C-49)

The user has no SharePoint yet, so it can be emulated only in parts.

| Part | Emulated by | What it proves |
|---|---|---|
| Graph file calls: list, download, upload, change tracking (delta), throttling and errors | Microsoft Dev Proxy (open source, runs on macOS), which intercepts ordin's calls to Microsoft Graph and returns prepared responses, errors and rate limits | ordin's Graph client and its error paths, repeatably and offline |
| Real response shapes | Graph Explorer's read-only sample tenant | The prepared responses match the real ones |
| Real sign-in and consent | A Microsoft 365 trial or developer tenant, if one can be obtained (eligibility to check) | Device sign-in and consent work in general |
| Company rules | **Company only** | Whether Conditional Access blocks device sign-in |

Dev Proxy's install route and plug-in names are checked at install time and recorded here, not assumed.

## 8. Results, January 2026

Working records: the lab's `RESULTS.md` files (outside the repository). The summary for each check is in 10-risks.md.

| # | Result |
|---|---|
| L1 | Done: 256 or later in Debian 13, Ubuntu 26.04, AlmaLinux 10 and Fedora 42; older in Debian 12, Ubuntu 22.04 and 24.04, AlmaLinux and Rocky 9 |
| L2 | Pass, with a change: `LoadCredentialEncrypted=` fails in a user unit, so ordin decrypts its key itself; the file is created mode 644, so ordin sets 0600 |
| L3 | Pass while polkit runs; otherwise an administrator enables lingering once |
| L4 | Pass |
| L5 | Pass |
| L6 | Deferred: no Copilot account |
| N1–N6 | Pass; `ws://` by host name to recheck on a dual-stack server; ordin enables retry of the first connection |
| N7 | Confirmed: proxy variables are ignored |
| D1 | Done |

**Deferred: needs another machine, an account the user doesn't have, or the company's own settings.**
- L6, the Copilot sign-in, waits for a Copilot account. A company plan's administrator can also switch the command-line tool off, so ask IT.
- D2, the Windows `tg:` handler, needs a Windows PC.
- Section 7: the real Microsoft sign-in and SharePoint, and the company's Conditional Access rules.
- The company's NATS `max_payload`, its Linux distributions and its Slack app-approval rule.
- D3, dictation on macOS 26: this Mac runs 14.6.1.
- The Graph mock in section 7 waits for ordin's Graph client to exist.

**Ready on this Mac, waiting for the user:**
- section 4, Telegram, waits for a throwaway bot;
- section 5, Slack, waits for a free workspace;
- the Claude Code sign-in in the test machine waits for the user to allow the install.

