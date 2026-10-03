# Kongwe (公威輸入法)

AI voice dictation for Traditional Chinese (Taiwan), English, and Japanese. Offline or self-hosted, with user-editable prompts.

## Layout

- `core/` Rust core crate `kongwe-core` (Apache-2.0)
- `apps/desktop/` Tauri 2 desktop client (macOS first, Apache-2.0)
- `apps/ios/` iOS client (planned)
- `server/` self-hosted engine server (planned, AGPL-3.0)

## Engines and models

Kongwe can run speech recognition and text rewriting on the machine with downloaded model files. Manage them in Settings > Engines and models. On macOS 26, Apple's built-in dictation and language model can be used with no download; the language model requires Apple Intelligence to be turned on. The Apple dictation language is installed from the Apple Dictation engine in Settings > Engines and models with the "Install language" button and is shared with the system.

Model files are stored in the models directory shown on the Engines and models page. Custom files can be imported: ggml `.bin` for speech recognition, GGUF for text. Each model's license is shown in the model list.

## Profiles, dictionary and speed test

The Profiles page edits one profile at a time; the menu at its top lists every profile with the applications it is used in, and has a search field when there are many. Profiles > "Add profile" creates a blank profile or one from a preset: mail, chat or code. A preset copies the engines of the default profile and pre-fills the application matchers and a prompt addition for that kind of application. All fields can be edited afterwards.

A dictionary term can span several lines. When one of its aliases is spoken, the whole term is inserted as written. For terms of this kind, their aliases are sent as the speech hint in place of the text, and the term is left out of the term list given to the language model.

The Speed test page, opened from the link at the foot of the Engines and models page, records a sentence, or uses a kept history recording, and runs it through the selected engines with a cold run and a warm run each. It measures speed only. Results are not saved.

## Permissions and password fields

The macOS client needs the Accessibility permission to insert text and the Microphone permission to record. When the Accessibility permission is taken away while Kongwe runs, the shortcuts stop at once and the menu-bar menu names the problem; they work again as soon as the permission is given back.

While macOS reports Secure Input (a password field has the keyboard), Kongwe does not insert text; a panel next to the widget shows the result with a Copy button. This depends on the application turning Secure Input on. Terminal does so for password prompts; in iTerm2 it is the "Secure Keyboard Entry" option; a terminal that never turns it on cannot be told apart from ordinary typing, so text dictated at its password prompt is inserted.

The recording shortcut is the fn key at first. Settings > Recording takes any number of shortcuts, recorded by pressing them: a modifier key alone (left and right are different keys), several modifiers, modifiers with one more key, or a mouse button other than the left and right ones. A combination or mouse button that is a shortcut is not passed on to the application in front. Letters, digits and symbols alone, combinations the system or most applications use, Esc and Caps Lock cannot be shortcuts. Every shortcut works the same way: hold to speak, press twice quickly for hands-free dictation.

The settings are on four pages: General (language, launch at login, permissions, the floating widget), Recording (shortcuts, microphone, recording length), Data and privacy (history, recordings, statistics, the context given to the rewrite) and Advanced (timeouts, retries, the insertion method per application). The menu of the menu-bar icon shows the state, copies the last result, and opens the history, the statistics and the settings; when the state is a problem, the item under it opens the place to resolve it.

A dictation that fails is explained in a panel next to the widget, with its detail and a Copy button, until it is dismissed or the next dictation starts. Hands-free dictation can end by itself after a pause: Settings > Recording. The floating widget has three sizes in the same place. While it listens, outlines around its ring and bars behind its label follow the input level. The ring's colour tells how a dictation ended: green when the text was inserted, amber when the text exists but needs attention (it was not inserted, or the rewrite did not succeed), red for a failure. The widget and the panel follow the light or dark appearance of the system. While a dictation is processed, the panel shows the recognised text and then what the processing changed in it; this can be switched off there too.

Requests to an engine over the network are tried again after a connection failure, a server error or a timeout; the number of retries is set in Settings > Advanced. When recordings are kept, the recording is written as soon as the shortcut is released, before it is processed, so a failure at any later point, or the app ending in the middle, leaves it in the history to be processed again; a dictation without speech or a cancelled one leaves nothing. With recordings switched off nothing is written, and a failed dictation cannot be processed again. Recordings are kept next to the history; Settings > Data and privacy can move them to another folder and opens that folder. When recordings are kept, a dictation that failed is listed in the history with its recording and can be processed again there. Any history row with a recording can be run again with the same or another profile, and the new result replaces the text only when accepted.

## License

Layered open core. The Rust core and the desktop and iOS clients are Apache-2.0 (see `LICENSE` and `core/LICENSE`). The self-hosted engine server under `server/` is AGPL-3.0 and carries its own license file. The hosted service's operations layer is not part of this repository.

## Trademark

"Kongwe" and "公威輸入法" are trademarks of the project owner. Forks must use a different name and logo.
