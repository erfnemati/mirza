# AI revision

Mirza can send what you said to an AI model before typing it. The model cleans it up and
nothing more:

- drops fillers and hesitations (um, uh, «اِ», «خب», «یعنی» when they mean nothing);
- drops words repeated by mistake («و و و», "the the");
- keeps only the last version when you correct yourself ("at three, no, at four" becomes "at
  four");
- adds punctuation, and Persian half-spaces where they belong («می‌خوام», «کتاب‌ها»);
- keeps every word in the language you spoke it in. It never translates, and colloquial
  Persian stays colloquial.

It is off by default and has its own shortcut, so normal dictation stays as fast as before. If
the AI is slow, fails, or sends back something that doesn't look like your text (much longer,
much shorter, or in another language), Mirza types what you said, unrevised, and tells you why.

## Turning it on

1. Open the settings and go to **AI revision**.
2. Pick a provider and paste its key (see below). We recommend Claude with its default model,
   Haiku. A local server needs no key.
3. Go to **Shortcuts** and set **Dictate and revise** (press once to start, again to stop) or
   **Hold, then revise**. There's no default key for these.
4. Try it from the **Try it** box at the bottom of the AI revision page: type or paste a
   sentence the way you'd say it, fillers and all, and click **Revise**.

From a terminal, `mirza toggle-revise` does the same as the shortcut, which is handy for
binding a key in your desktop's own keyboard settings.

The text is typed once the revision is back, usually in under a second for a sentence and a
few seconds for a long paragraph. Stay in the window until then. Where Mirza can see which
window is active (Windows, and KDE Plasma on Linux), switching to another window sends the text
to the clipboard instead; elsewhere it is typed wherever you are.

**Recent text** shows both versions, so you can copy what you actually said if the revision
went wrong.

## Privacy

With AI revision on, the text of what you said (not the audio) goes to the AI provider you
picked, on top of the speech service. A local server keeps it on your own computer or network.
Normal dictation, without the revise shortcut, doesn't use the AI at all.

## Providers

| Provider | Default model | Key | Environment variables |
|---|---|---|---|
| Claude (recommended) | `claude-haiku-5-5` | [console.anthropic.com/settings/keys](https://console.anthropic.com/settings/keys) | `MIRZA_CLAUDE_API_KEY`, `ANTHROPIC_API_KEY` |
| OpenAI | `gpt-6-luna` | [platform.openai.com/api-keys](https://platform.openai.com/api-keys) | `MIRZA_OPENAI_API_KEY`, `OPENAI_API_KEY` |
| Gemini | `gemini-3.5-flash-lite` | [aistudio.google.com/apikey](https://aistudio.google.com/apikey) | `MIRZA_GEMINI_API_KEY`, `GEMINI_API_KEY`, `GOOGLE_API_KEY` |
| OpenRouter | `openai/gpt-6-luna` | [openrouter.ai/settings/keys](https://openrouter.ai/settings/keys) | `MIRZA_OPENROUTER_API_KEY`, `OPENROUTER_API_KEY` |
| Local server | the first one the server lists | none, or your server's own | `MIRZA_LOCAL_API_KEY` |

The defaults are small, fast models (checked in November 2026); cleaning up dictation doesn't
need a big one. The model field on the settings page lists what your key can use, and you can
type any other name.

Keys are saved the same way as speech keys: in the settings window, with
`mirza set-key <provider>` (reads the key from standard input), or in an environment variable.

Notes per provider:

- **Gemini**: isn't offered in some countries; if you get a message about your region, use a
  proxy in a country it serves. Check Google's terms for whether free-tier text is used to
  improve its models.
- **Claude**: API keys and credit come from the Anthropic Console, separate from a Claude.ai
  subscription.
- **OpenRouter**: one key for models from many companies. Model names include the company,
  like `openai/gpt-6-luna` or `anthropic/claude-haiku-5.5`. Mirza shows up as the app on your
  OpenRouter activity page.
- **Local server**: anything that speaks OpenAI's Chat Completions API: vLLM, llama.cpp
  (`llama-server`), Ollama, LM Studio. Set the address ending in `/v1`, for example
  `http://localhost:8000/v1` (vLLM), `http://localhost:11434/v1` (Ollama) or
  `http://localhost:1234/v1` (LM Studio). Leave the model empty to use the first one the server
  lists. Small models are more likely to miss half-spaces or to translate mixed sentences, so
  try yours in the Try it box first.

## Proxy

Like every other call Mirza makes, AI requests go through the proxy in **Settings > General**.
A local server is the exception: an address on your own computer or network is reached
directly, so the proxy (which usually can't see your network) doesn't get in the way. These
count as local:

- `localhost` and loopback addresses (`127.0.0.1`, `::1`);
- private network addresses (`10.x.x.x`, `172.16–31.x.x`, `192.168.x.x`, IPv6 `fc00::/7`) and
  link-local ones (`169.254.x.x`, `fe80::`);
- names without a dot (`gpu-box`) and names ending in `.local`, `.lan`, `.home.arpa`,
  `.internal` or `.localhost`.

Any other address, including a cloud provider's own, goes through the proxy. If your local
server is on a public address or a domain like `ai.example.com`, it goes through the proxy too.

## Other settings

- **Thinking** (under More options): how much a reasoning model may think before answering.
  It is off by default for OpenAI, Claude and OpenRouter, because it adds seconds and rarely
  helps with cleanup; Gemini and local servers use the model's own default. If a model rejects
  the setting, choose **The model's default**.
- **Time limit**: how long to wait for the AI, 10 seconds by default. Longer dictations get a
  second more for every 30 words over 100. When the time is up, the unrevised text is typed.
- **Your own rules**: anything else you want, in plain words, for example "Write numbers as
  digits" or "Use formal Persian". These win over Mirza's own rules.

The custom words from your speech service settings are passed on too, so names are spelled
the way you want them.

In the settings file these are the `[revision]` and `[ai.<provider>]` sections; the template
Mirza writes on first start explains each key.

## When something goes wrong

Mirza types what you said and shows a notification that starts with "Typed without AI
revision", followed by the reason. The common ones:

- **No API key, or the key wasn't accepted**: paste the key again in Settings > AI revision.
- **Out of credit or quota, or too many requests**: add credit on the provider's site, or wait a
  moment.
- **Doesn't have the chosen model**: pick another model from the list.
- **Isn't offered in your country**: set a proxy in a country the provider serves.
- **Took too long to answer**: raise the time limit, choose a faster model, or set Thinking to
  None.
- **The answer didn't look like your text**: the model answered your words instead of cleaning
  them, or translated them. Try another model.
- **Couldn't reach your local AI server**: check that the server is running and the address is
  right.

Run `mirza` from a terminal with `MIRZA_LOG=mirza=debug` to see each request's provider,
model and timing, and the text before and after. Without `debug`, the log never shows your
text or your keys.
