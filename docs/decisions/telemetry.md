# Telemetry: anonymous usage statistics for Termoil, Chordical and the Chordical Synthesizer

task-2530. This is a design reading, not a measurement. It says what is built and why, for three
products, and the TDD (section 4.11) and the PRD are updated to match for Termoil.

## The ask

> We want to add anonymous collection of usage data for termoil, so we can understand how many users
> are using it, what features they are using etc.
>
> I want the same for our Chordical DAW, and Chordical vst.
>
> Look at how we set things up previously, eg with Unluminous.
> We want it on by default with an option to disable telemetry.

The previous design is Unluminous task-2271 (`C:/jason/dev/unluminous/tasks/task-2271-telemetry-tdd.md`,
code in `crates/unluminous-app/src/services/telemetry/`). This design copies it, and the sections below
say where each product differs.

## 1. What was set up in Google Analytics

Three GA4 properties in the account `Inillucent.com` (`a407371481`), beside `Unluminous App`, each with
one web stream and one Measurement Protocol API secret. Creating a secret required accepting Google's
*User Data Collection Acknowledgement* on each property, as it did for Unluminous. A test event posted to
`/debug/mp/collect` came back with no validation messages for all three.

| Product | GA4 property | Stream | Measurement id | API secret nickname |
|---|---|---|---|---|
| Termoil | `Termoil App` (563016471) | `Termoil App`, `https://unluminous.com/termoil`, 16289426507 | `G-NDVVCGE4FD` | `Termoil desktop app` |
| Chordical | `Chordical DAW` (563056517) | `Chordical DAW`, `https://chordical.com`, 16289507222 | `G-52TK618MY2` | `Chordical desktop app` |
| Chordical Synthesizer | `Chordical VST` (563056921) | `Chordical VST`, `https://chordical.com/vst`, 16289486169 | `G-XCK6BDEX4Q` | `Chordical VST plugin` |

A separate property for each product keeps one product's users and events out of another's reports.
The synthesizer has its own property even though Chordical ships it, because the synthesizer also runs
in other hosts (Ableton Live, Reaper, FL Studio), and "how many people use the synthesizer" is a
different number from "how many people use Chordical".

### Why the Measurement Protocol

All three are native programs, so `gtag.js` cannot run in them. The GA4 Measurement Protocol is an HTTPS
`POST` of a small JSON body to `https://www.google-analytics.com/mp/collect?measurement_id=…&api_secret=…`.
It needs a web stream's measurement id and an API secret, and nothing else. Unluminous section 1 compares
the other two routes (Firebase, a hidden web view) and rejects both; the same reasons hold here.

The API secret is compiled into each binary. A Measurement Protocol secret lets its holder send events to
that one stream and read nothing. If one is abused it is rotated in the GA admin page and the next release
carries the new one.

## 2. The rules every product follows

- **On by default.** One switch turns it off completely, and off means nothing is counted and nothing is
  sent from that moment on, including the closing event. What was counted and not yet sent is thrown
  away when the switch goes off.
- **What is sent is a closed list of names** chosen by the program: action names from the catalogue or
  menus, command wire names, panel names, file kinds, shell kinds, a few counts and durations. No path,
  file name, project name, preset name, plugin name, typed text, command argument, URL, address, user name
  or machine name is ever sent. Every name passes `clean_name`, which drops a name with a character outside
  the identifier set rather than sending it.
- **A process something else is driving sends nothing**: a window started with `--background` or
  `--headless`, a benchmark run, and every test. An environment variable turns it off for a process
  (`<PRODUCT>_TELEMETRY=off`), and `on` turns it on for a process that would otherwise send nothing.
  `debug` is `on` plus GA4's `debug_mode` (events show in DebugView) plus a copy of every body in
  `telemetry/sent.jsonl`.
- **The client id** is 32 hexadecimal characters from the operating system's random source, made once and
  kept in `telemetry/id.txt`. Nothing about the machine or the person goes into it. Deleting the file makes
  a new one.
- **Nothing waits on the network.** Counting is one relaxed atomic load while telemetry is off, and one
  uncontended lock and one map increment while it is on. Posting is on a thread of its own. Closing waits
  at most 600 ms, and only after the closing bodies are written to the spool.
- **What could not be posted is kept, not retried in a loop.** A failed body goes to the spool, and the next
  start sends it. A body older than 72 hours is dropped, because GA4 refuses a `timestamp_micros` older
  than that. A spool keeps its newest 200 lines.
- **Request budget.** The first request goes 10 seconds after start. After that, one request every 20
  minutes while something was counted or somebody was active, none while idle, and one on close. At most
  25 events a request (GA4's limit), at most 4 requests at once, at most 200 requests in a process's life.
  An eight hour working day in one window is about 26 requests.

## 3. The events

The same names in all three products, so one set of GA reports reads each property the same way. GA4
reserves `session_start`, `first_visit`, `first_open`, `user_engagement` and `screen_view`; none of these
collide with them.

| Event | When | Parameters |
|---|---|---|
| `app_open` | in the first request of a process | product specific, listed per product below; always `first_run` (1 the first time this installation sends anything) |
| `app_heartbeat` | in every request | `engagement_time_msec`: active time since the last request |
| `feature` | one per distinct name used since the last request | `category`, `feature` (the name), `uses` (how many times) |
| `panel_time` | one per panel or view that was showing while somebody was active | `panel`, `seconds` (Chordical only) |
| `app_close` | in the last request of a process | `open_seconds`, `focused_seconds`, `active_seconds`, plus product specific counts |
| `app_crash` | in the first request after a start that finds a new crash report | `uses` (always 1) |

Every event carries `session_id` (the Unix time in seconds the process started) and
`engagement_time_msec`. Every request carries `user_properties` (`app_version`, `os`, `arch`) and a
`device` object (`category: desktop`, `operating_system`).

**Open, focused and active are three different numbers.** *Open* is wall time from start to close.
*Focused* is time a window held the operating system's focus. *Active* is focused time with input in the
last 60 seconds, which is GA4's own idea of engagement.

## 4. Termoil

### Where the code already funnels what a person does

| Funnel | What goes through it | Counted as |
|---|---|---|
| `App::run_action` (`app.rs`) | Every keybinding, the command palette, the mouse's tab buttons, the settings view, `termoil +command`, IPC and the MCP server | `category: action`, the catalogue action name |
| `UserEvent::Ipc` in `gui/mod.rs` | The subset of actions that came from `termoil +command`, a script, or an agent over the socket | `category: ipc`, the action name, in addition to the `action` row |
| `App::spawn_session` (the one place a shell starts) | Every new tab, split and window | `category: shell`, the shell kind from `shell_integration::detect` (`pwsh`, `powershell`, `cmd`, `bash`, `zsh`, `fish`, `nushell`, `other`) |
| `ConfigChanged` | The configuration was reloaded | `category: config`, `reload` |

`termoil_protocol::action` already refuses a name the catalogue does not have before `run_action` does
anything, so only catalogue names are ever counted.

### Termoil's `app_open` and `app_close`

| Event | Extra parameters |
|---|---|
| `app_open` | `launch` (`plain`, `folder`, `command`, `restore`), `renderer` (`gpu`, `cpu`), `theme` (a bundled theme name or `other`), `channel` (`stable`, `nightly`) |
| `app_close` | `most_windows`, `most_tabs`, `most_panes`: the largest number open at once during the process |

### No frame clock

Termoil's event loop waits (`ControlFlow::Wait`) and draws only when something changed, so it has no
frame to count time on. The clocks are driven by events instead: `telemetry::input(now)` on every key,
mouse button, wheel and IME event, and `telemetry::focus(focused, now)` on `WindowEvent::Focused`. Active
time is the length of each interval from an input to the next input, capped at 60 seconds, counted only
while a window has the focus. `Activity` is a plain struct of times with pure methods, so it is tested
with made up instants.

Sending is driven by the telemetry thread, which waits on its job channel with a timeout of the time until
the next request is due. The window's thread never builds a body.

### Transport

Termoil keeps an HTTP client and a TLS stack out of its build (`docs/decisions/updates.md`, "Where a
check asks"), so a request is `curl` from System32 on Windows and `/usr/bin/curl` elsewhere, the same
program the update check uses: `update::fetch::post_json` sends the body on curl's standard input with
`--data-binary @-`, a 10 second `--max-time`, and no console window.

### Setting, action and environment

- `[telemetry]` in the configuration file: `enabled = true`, and `interval_minutes = 20` (the tunable
  number; the GA4 limits are constants because they are Google's). The settings view shows both, because
  it lists every key in the schema. A configuration reload turns telemetry on or off from that moment.
- `telemetry-status` in the catalogue: prints whether it is on, why it is off when it is (`setting`,
  `environment`, `headless`, `benchmark`, `not started`), the client id, requests posted and failed, rows
  waiting, and the last failure. `--flush true` sends what is waiting now, which is how the end to end
  check is driven without waiting twenty minutes. It is a `termoil +telemetry-status` command, an MCP tool,
  and a palette entry, generated from the catalogue like every other action.
- `TERMOIL_TELEMETRY` = `off`, `on` or `debug`; `TERMOIL_TELEMETRY_URL` points the sender at a scripted
  server on loopback. The end to end tests and the benchmark tool set `TERMOIL_TELEMETRY=off` on every
  `termoil-gui` they start. A `--headless` instance sends nothing unless the environment says `on`.

### Files

The data folder's `telemetry/` holds `id.txt`, `spool.jsonl` (bodies not yet posted) and `crashes.txt`
(how many crash reports `crashes/` held at the last start; more means `app_crash`). One Termoil process
serves every window on a socket, so one spool per data folder is enough: a starting process claims it by
renaming it to `sending-<pid>.jsonl`, and a `sending-` file whose process is no longer running is claimed
again.

| File | What it holds |
|---|---|
| `crates/termoil-app/src/telemetry/mod.rs` | `note`, the tally, the switch, `Permission`, the client id, the crash count |
| `crates/termoil-app/src/telemetry/activity.rs` | `Activity`: open, focused and active time from events |
| `crates/termoil-app/src/telemetry/payload.rs` | GA4 bodies and their limits; pure |
| `crates/termoil-app/src/telemetry/sender.rs` | the thread, the spool, `telemetry-status` |
| `crates/termoil-app/src/telemetry/tests.rs` | the unit tests |

## 5. Chordical

Chordical's window, control layer and services were copied from Unluminous (its `CLAUDE.md`), so its
telemetry is the Unluminous module copied into `crates/chordical-app/src/services/telemetry/` with a header
naming the source path and revision, and `ureq` (already a dependency) as the transport.

As built (Chordical task-2530), copied from Unluminous at 9332c889c42894cd133e06ac72bfa80c5af77ccd:

| Funnel | Counted as |
|---|---|
| `ChordicalApp::run_action` (`app/menus.rs`) | `category: action`, `Action::name()` |
| `ChordicalApp::run_cli` (`app/cli.rs`) | `category: command`, the catalogue wire name (`transport.play`, `clip.add`); a name the catalogue does not have is not counted |
| `ChordicalApp::run_a_chat_command` (`app/agent_chat.rs`), a command the Agent Chat pane's model called as a tool | `category: agent_tool`, the wire name. The same call also reaches `run_cli` through the control server and is counted there as a `command`, so an `agent_tool` row is a subset of the `command` rows |
| `ChordicalApp::insert_plugin` (`app/devices.rs`), the one place a device is put on a track | `category: device`, `plugin:vst3`, `plugin:clap` or `plugin:vst2`, and `builtin` for the test delay and other devices that are not third party plugins. A plugin's name is never sent |
| The view that is showing, sampled once a second while somebody is active | `panel_time` for `arrangement` or `session` (the main view), each open panel (`browser`, `mixer`, `clip-view`, `agent-chat`) and `settings` while the Settings window is open |

`app_open` carries `launch` (`plain`, or `project` when a project file was named on the command line),
`theme` (a bundled theme key or `other`), `audio_driver` (`wasapi`, `asio`, `coreaudio`, `none`, read from the
`audio.host` setting) and `first_run`. `app_close` carries `most_tracks`, the most tracks the open project had
at once. There is no `app_crash`, because Chordical keeps no crash log to compare. The `channel` parameter
Termoil sends is not in Chordical.

The switch is `telemetry.enabled` (default true; on when the settings file has no value), a **Usage
statistics** page in the Settings window with one switch, *"Send anonymous usage statistics"*, and a sentence
saying what is and is not sent. It is on `settings list`, `settings get` and `settings set`, and on
`chordical-cli settings telemetry [--flush]`, which reports whether the window sends, why not when it does not
(`allowed`, `environment`, `background window`, `test`), the client id, requests posted and failed, and how many
counted names are waiting. A `--background` window sends nothing unless `CHORDICAL_TELEMETRY=on` or `debug`;
`CHORDICAL_TELEMETRY=off` stops every window. A `--headless` host has no frames to drive the clocks, so it never
starts telemetry. A window a test builds has permission `test` and sends nothing. `CHORDICAL_TELEMETRY_URL` and
`CHORDICAL_TELEMETRY_EVERY` are the test seams. The files are in `telemetry/` beside `settings.conf`: `id.txt`,
`spool-<pid>.jsonl`, and `sent.jsonl` in debug mode. The transport is `ureq`, with the TLS configuration the
Agent Chat client uses.

## 6. The Chordical Synthesizer

The synthesizer is a VST3 plugin in C++ with JUCE and a React interface in a web view. It differs from the
two programs in four ways.

1. **It runs inside somebody else's process**, and a host may load many instances. Telemetry is one
   process wide object (`juce::SharedResourcePointer<chordical::Telemetry>`), so a host with eight
   instances sends one stream of requests, and `instances` in `app_close` says how many were loaded at once.
2. **It must never touch the audio thread.** Nothing in `processBlock` counts anything. Counting happens on
   the message thread (the web bridge's RPC dispatch and the editor's lifetime), and posting on a
   `juce::Thread` of its own.
3. **Its "window" is the editor.** *Open* is time from the first instance to the last instance going away;
   *focused* is time an editor was open; *active* is editor time with an RPC from the interface in the last
   60 seconds.
4. **The transport is `juce::URL::withPOSTData(...).createInputStream(...)`**, which uses WinINet on
   Windows and NSURLSession on macOS, so no new library is linked.

| Funnel | Counted as |
|---|---|
| The web bridge's RPC dispatch (`WebBridge`) | `category: rpc`, the registered RPC name; only names in the registration table are counted, and polling RPCs (meters, the scope, performance metrics) are left out |
| A preset loaded | `category: preset`, `factory` or `user`, never its name |
| A tab of the interface shown (Synth, Keyboard) | `panel_time` |

`app_open` carries `host` (`juce::PluginHostType`'s name for a known host, `other` otherwise), `format`
(`vst3`, or `standalone` for the standalone build), and `first_run`. `app_close` carries `instances`.

### What was built (chordical-vst-2026, branch `task-2530`)

| File | What it holds |
|---|---|
| `plugins/synthesizer/src/telemetry/TelemetryPayload.{h,cpp}` | GA4 bodies and their limits; pure |
| `plugins/synthesizer/src/telemetry/TelemetrySender.{h,cpp}` | `postBody` (`juce::URL::withPOSTData` and `createInputStream`), the spool, claim by rename, `secureRandomBytes` |
| `plugins/synthesizer/src/telemetry/Telemetry.{h,cpp}` | the tally, the switch, the clocks, the client id, the `juce::Thread`, `Options` |
| `plugins/synthesizer/src/web/TelemetryRpc.{h,cpp}` | the `getTelemetry`, `setTelemetryEnabled` and `setTelemetryPanel` RPCs |
| `tests/cpp/TelemetryTest.cpp` | the unit tests and the loopback server test, run by `SynthAudioTests` |

Where counting happens, and what differs from the sketch above:

- **RPCs** are counted in `RpcRecorder::wrapHandler`. Every entry in `WebBridgeRegistration.cpp` is
  registered as `RpcRecorder::Tap{"name"} | handler`, so that one function is the funnel for all of
  them, and the name counted is the registered name. `isQuietRpc` skips `testHarness/*`, every name
  starting with `get`, `eqzGetDynamicMeters`, the list and waveform reads, `graphSetViewport`,
  `graphSetWindowSize`, `graphSetNodeSize`, the three telemetry RPCs, and the three RPCs the interface
  sends by itself when it loads (`setKeyboardNotesEnabled`, `setSuppressedMidiNotes`,
  `midiLearnRegisterSelect`). A skipped RPC neither counts nor marks the interface as in use. Calls to one RPC less than 0.3 seconds apart are one gesture, so a
  knob drag counts once and not hundreds of times (`gestureGapSeconds`).
- **Preset loads** are counted in `handlePresetLoad` as `preset` / `factory` or `user`. A preset is
  `factory` when it is in bank 1 or its author is `Factory`.
- **Instances** are counted by `SynthProcessor`'s constructor and destructor. The editor's constructor and
  destructor start and stop focused time.
- **`panel_time`** comes from the `setTelemetryPanel` RPC, which `App.jsx` calls with `synth` or `keyboard`
  whenever the tab changes. Only those two values are accepted.
- **`app_crash` is not sent.** The synthesizer has no crash report folder or crash log to look for, and
  a plugin cannot tell a host crash from a plugin crash.
- **One object per plugin binary.** `juce::SharedResourcePointer` is shared inside one module, so two
  different copies of the plugin loaded into one host each send their own stream.
- **A host that only scans the plugin sends nothing.** A process that never opened an editor, never sent a
  request and closes within `firstAfterSeconds` (10) is a plugin scan, so its closing request is not sent.
- **Test builds send nothing.** Every executable in `tests/cpp` is compiled with
  `CHORDICAL_TELEMETRY_TOOL`, which makes the default `off` unless `CHORDICAL_SYNTH_TELEMETRY` says `on`
  or `debug`. `SynthAudioTests` and `ChordicalTestHarness` also set `CHORDICAL_SYNTH_TELEMETRY=off` for
  themselves unless it is already set.
- **Folders** follow `CHORDICAL_APPDATA_ROOT` when it is set (the plugin's other files already do), so
  `settings.xml` and `telemetry/` can be pointed at a scratch folder.

The switch is `telemetryEnabled` (default true) in the synthesizer's global settings file
(`ChordicalSynthesizer/settings.xml` in the user's application data folder, beside the existing preset
file), not in a preset or the host's project, so it holds for every instance in every host. It is a row in
the interface's **Settings** modal (the gear in the preset bar): *"Send anonymous usage statistics"*, with
the sentence saying what is and is not sent, backed by the `getTelemetry` and `setTelemetryEnabled` RPCs.
`CHORDICAL_SYNTH_TELEMETRY=off|on|debug` and `CHORDICAL_SYNTH_TELEMETRY_URL` are the environment seams. The
test harness and the C++ test executable set it to `off` for themselves unless the environment already says
otherwise, so a test run sends nothing.

## 7. How it is verified

1. Unit tests in each product: the payload builder (25 events, 40 character names, 100 character values,
   overflow carried to the next request), the clocks, the spool (claiming, 72 hour expiry, cutting), and
   that a name with a path or a third party plugin's name is never sent.
2. A test with a scripted HTTP server on loopback: telemetry on sends `app_open`, a `feature` row for an
   action that ran, and `app_close`; the setting off sends nothing.
3. The installed build of each product, with the environment set to `debug`: the events reach GA4's
   `/debug/mp/collect` with no validation messages, and show in the Realtime report of each property.

## 8. Left out

- **Error messages.** A crash is counted; its text stays on disk, because a panic message can hold a path.
- **Timing of individual features.** Counts and time answer the ticket.
- **A consent prompt on first start.** The ticket asks for on by default with an option to turn it off.
- **Linking the three properties.** A person who uses Chordical and the synthesizer inside it is two
  clients, because the two do not share an id. Sharing one would mean the plugin reading the DAW's files,
  and the plugin also runs in other hosts.
