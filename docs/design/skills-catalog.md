# Skills catalog (draft 1, 2026-11-24)

**Status:** a starting list, not final. The owner has about 200 skills in mind and an earlier project with 181 script
skills that has not been read yet (Phase 14 §9). This file is where they get merged into one list: **one name per
action, one tier, one class.** Plan: `docs/plans/phase-14-skills-library-plan.md`.

Tier key: **S** = SAFE, **R** = REVIEW (dialog shows exactly what will happen), **D** = DANGEROUS (always asks, shows
the exact payload, may need Touch ID). "u" after a tier means an undo is stored. Names use `group.action`.
Counts are targets per group; the examples are the first ones to build.

## Pack A — System (about 45)

| Group | Count | Examples (tier) |
| :-- | --: | :-- |
| `audio` | 8 | `audio.volume_set` (S,u), `audio.volume_change` (S,u), `audio.mute` (S,u), `audio.unmute`, `audio.output_device_set` (S,u), `audio.input_device_set` (S,u), `audio.input_mute` (S,u), `audio.read_state` (S) |
| `display` | 8 | `display.brightness_set` (S,u), `display.brightness_change` (S,u), `display.dark_mode` (S,u), `display.night_shift` (S,u), `display.true_tone` (S,u), `display.sleep_now` (S), `display.keep_awake` (S,u), `display.read_state` (S) |
| `network` | 6 | `network.wifi_on_off` (R), `network.wifi_name` (S), `network.join_known` (R), `network.speed_test` (R, uses the fetcher), `network.bluetooth_on_off` (R), `network.read_state` (S) |
| `power` | 6 | `power.battery` (S), `power.low_power_mode` (S,u), `power.sleep` (R), `power.lock_screen` (S), `power.restart` (D), `power.shutdown` (D) |
| `focus` | 5 | `focus.dnd_on_off` (S,u), `focus.set_mode` (S,u), `focus.until` (S,u), `focus.read_state` (S), `focus.notification_summary` (S) |
| `screen` | 5 | `screen.screenshot_full` (S), `screen.screenshot_area` (S), `screen.screenshot_window` (S), `screen.record_start_stop` (R), `screen.read_text` (R, uses screen content) |
| `system` | 7 | `system.metadata` (S, exists), `system.disk_report` (S, exists), `system.cleanup` (R, exists), `system.processes_top` (S), `system.quit_runaway` (R), `system.empty_trash` (D), `system.uptime` (S) |

## Pack B — Apps, windows, clipboard, notifications, files (about 45)

| Group | Count | Examples (tier) |
| :-- | --: | :-- |
| `app` | 8 | `app.open` (S), `app.quit` (R), `app.force_quit` (D), `app.switch` (S), `app.hide` (S), `app.list_running` (S), `app.install_info` (S), `app.open_with_file` (S) |
| `window` | 8 | `window.tile_left` (S,u), `window.tile_right` (S,u), `window.maximize` (S,u), `window.minimize` (S), `window.move_to_desktop` (S), `window.list` (S), `window.close` (R), `window.fullscreen` (S,u) |
| `clipboard` | 5 | `clipboard.read` (R, content is tainted), `clipboard.write` (S), `clipboard.history_search` (R), `clipboard.clear` (S), `clipboard.paste_plain` (S) |
| `notify` | 6 | `notify.send_self` (S), `notify.read_recent` (S), `notify.clear_all` (R), `notify.dismiss_app` (S), `notify.mute_app` (R), `notify.summarize_today` (S) |
| `files` | 18 | `files.find` (S), `files.open` (S), `files.preview` (S), `files.reveal` (S), `files.recent` (S), `files.size` (S), `files.copy` (R), `files.move` (R), `files.rename` (R), `files.zip` (R), `files.unzip` (R), `files.make_folder` (R), `files.tag` (R), `files.trash` (D), `files.duplicate_find` (S), `files.downloads_tidy` (R), `files.open_folder` (S), `files.share_link_copy` (R) |

## Pack C — Mail, calendar, reminders, notes, messages (about 40)

| Group | Count | Examples (tier) |
| :-- | --: | :-- |
| `mail` | 12 | `mail.unread_count` (S), `mail.list_unread` (S), `mail.read` (R, content tainted), `mail.search` (S), `mail.summarize_thread` (S), `mail.draft` (R), `mail.reply_draft` (R), `mail.flag` (R), `mail.archive` (R), `mail.mark_read` (R), `mail.send` (D), `mail.delete` (D) |
| `calendar` | 10 | `calendar.read` (S, exists), `calendar.today` (S), `calendar.next` (S), `calendar.free_time` (S), `calendar.create_event` (R), `calendar.move_event` (R), `calendar.cancel_event` (D), `calendar.invite` (D), `calendar.set_alert` (R), `calendar.join_meeting_link` (R) |
| `reminders` | 6 | `reminders.add` (R), `reminders.list` (S), `reminders.complete` (R), `reminders.snooze` (R), `reminders.delete` (D), `reminders.add_location_free` (R) |
| `notes` | 6 | `notes.search` (S, exists), `notes.append` (R, exists), `notes.create` (R), `notes.read` (S), `notes.pin` (R), `notes.delete` (D) |
| `messages` | 6 | `messages.unread` (S), `messages.read` (R, tainted), `messages.draft` (R), `messages.send` (D), `messages.call_start` (D), `messages.mute_chat` (R) |

## Pack D — Browser, media, shortcuts (about 35)

| Group | Count | Examples (tier) |
| :-- | --: | :-- |
| `browser` | 14 | `browser.open_url` (R if the URL came from tainted text, else S), `browser.search` (S), `browser.new_tab` (S), `browser.close_tab` (R), `browser.switch_tab` (S), `browser.list_tabs` (S), `browser.reload` (S), `browser.back` (S), `browser.bookmark_add` (R), `browser.bookmark_open` (S), `browser.reading_list_add` (R), `browser.read_page_text` (R, tainted), `browser.private_window` (S), `browser.downloads_list` (S) |
| `media` | 10 | `media.play_pause` (S), `media.next` (S), `media.previous` (S), `media.now_playing` (S), `media.play_search` (S), `media.like` (R), `media.seek` (S), `media.shuffle` (S), `media.queue_add` (R), `media.stop` (S) |
| `shortcuts` | 6 | `shortcuts.list` (S), `shortcuts.run` (R by default; S when the owner marks one), `shortcuts.run_with_input` (R), `shortcuts.create_from_skill` (R), `shortcuts.schedule` (R), `shortcuts.disable` (S) |
| `homekit` (optional) | 5 | `home.lights` (S,u), `home.scene` (S,u), `home.thermostat` (R), `home.lock` (D), `home.read_state` (S) |

## Pack E — Dev, productivity, text, web (about 35)

| Group | Count | Examples (tier) |
| :-- | --: | :-- |
| `dev` | 10 | `dev.git_status` (S), `dev.git_diff_summary` (S), `dev.git_log` (S), `dev.run_tests` (R), `dev.build` (R), `dev.open_project` (S), `dev.explain_error` (S, takes screen context), `dev.lint` (R), `dev.docs_search` (S), `dev.git_commit` (D) |
| `clock` | 8 | `clock.timer_set` (S,u), `clock.timer_cancel` (S), `clock.alarm_set` (S,u), `clock.stopwatch` (S), `clock.world_time` (S), `clock.countdown_to` (S), `clock.pomodoro` (S,u), `clock.list` (S) |
| `text` | 9 | `text.calculate` (S), `text.convert_units` (S), `text.currency_cached` (S), `text.summarize_given` (S), `text.rewrite_given` (S), `text.translate_local` (S), `text.spell_check` (S), `text.word_count` (S), `text.define_local` (S) |
| `web` | 8 | `web.fetch` (R, exists), `web.search` (R, exists), `web.weather` (R), `web.news_headlines` (R), `web.define` (R), `web.translate_online` (R, gated), `web.package_info` (R), `web.status_check` (R) |

## Always-present core (exists today, 13)

`system.read_metadata`, `system.disk_report`, `calendar.read`, `notes.search`, `privacy.pause`, `profile.remember`,
`profile.forget`, `memory.forget`, `notes.append`, `system.cleanup`, `web.fetch`, `web.search`, `shell.exec` (disabled).

## Totals

Pack A about 45, B about 45, C about 40, D about 35, E about 35 = about 200, plus the 13 core skills.
Anything the 181-script project has that is not listed here is added in the import step (P14-S05); anything here that
the owner never uses is dropped. **Never build:** moving money, posting publicly, purchases, deleting accounts.

## Rules that decide a tier (so new skills need no argument)

- Reads only data that belongs to the machine's own state: **S**. Reads content that came from outside (mail,
  messages, web, clipboard): **R**, and its output is **tainted**.
- Changes one setting that is trivially undone and stores the old value: **S**.
- Changes your data, moves or renames files, drafts something, or opens a URL that came from tainted text: **R**.
- Deletes, sends to another person, changes security, restarts the machine, or runs code: **D**.
- A skill's tier is the **highest** of: its own base tier, the tier raised by tainted arguments, and the tier of
  anything it calls.
