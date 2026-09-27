# Notices

## Logos and trademarks

The names and logos below belong to their owners. Herdr Nudge is not
affiliated with, endorsed by or sponsored by any of them.

Each agent logo appears only on the right of a notification about a
terminal pane where that agent is running, to show which agent it is. The
Herdr logo is the notification's app icon, because the notification is
about your Herdr session. The files are unmodified copies of the sources
listed. Some logos have a second file in `assets/agents/dark/` for dark mode,
taken from the same source.

The screenshots in `assets/readme/` are of real notifications, so they show
the Herdr logo and the Kilo and Grok logos as a notification does.

If you own one of these marks and want it taken out, open an issue at
<https://github.com/justinchiasson/herdr-nudge/issues> and it will be removed.

### Herdr

| File | Owner | Source | sha256 |
|---|---|---|---|
| `assets/herdr-logo.png` | Herdr, Inc. | <https://herdr.dev/assets/logo.png> | `56fc2db845c16eb521022549890fbe239659957caee1f4fc718a634d7a66cf0a` |

The app icon `vendor/HerdrNudge.app/Contents/Resources/HerdrNudge.icns` is
built from that file by `tools/bundle/build.sh`.

### Agent logos

Unless a row says otherwise, the file comes from
[`@lobehub/icons-static-png`](https://www.npmjs.com/package/@lobehub/icons-static-png)
1.97.1 (LobeHub's redrawings of each logo, MIT licence below): the light file
from its `light/` directory and the dark file from `dark/`.
`tools/fetch-icons.sh` rebuilds the directory from these sources.

| File | Agent | Owner | Source | sha256 |
|---|---|---|---|---|
| `assets/agents/agy.png` | Antigravity | Google LLC | `light/antigravity-color.png` | `ac20d11802a405491048957d68c84f7303ceaf682a95cf890263452ccdd97897` |
| `assets/agents/amp.png` | Amp | Amp Frontier Corporation | `light/amp-color.png` | `b8eb270dc3fb0abe606b2efa041b21a7934c604f6af97758a8dcdab5e5bb4a84` |
| `assets/agents/claude.png` | Claude Code | Anthropic, PBC | `light/claude-color.png` | `97b53fb85b3bf401ffeb8ee7c5ebad734ce9f6edc3038f242021c53bcc18e23d` |
| `assets/agents/cline.png` | Cline | Cline Bot Inc. | `light/cline.png` | `129a9431855f4fd24321ae0ef9b393b85d13ac62c755715d1bf2be0064d42df3` |
| `assets/agents/dark/cline.png` | Cline | Cline Bot Inc. | `dark/cline.png` | `42abfb62eb7d439a896bc1f98302f004f72fb4ebd617ca58f6a1242af02dac8b` |
| `assets/agents/codex.png` | Codex | OpenAI | `light/codex-color.png` | `fcea9ddbaafdca236a8380cef2ecd3342ecd9914a7b080873873cf45f415686d` |
| `assets/agents/copilot.png` | GitHub Copilot | GitHub, Inc. | `light/githubcopilot.png` | `984fc3fb2708c83fcc870eabfb2a9a4cd4a2a059ac18f78200119dc37f95ce8c` |
| `assets/agents/dark/copilot.png` | GitHub Copilot | GitHub, Inc. | `dark/githubcopilot.png` | `a9b80430e3b70f7ae4e294a5c2548aa4a7e38e8d933dfd7d2051d14dac620d6d` |
| `assets/agents/cursor.png` | Cursor | Anysphere, Inc. | `light/cursor.png` | `783ac5ded2079c86a2741af24f1d168ea1b7d12846feea4bac54d79ffb061062` |
| `assets/agents/dark/cursor.png` | Cursor | Anysphere, Inc. | `dark/cursor.png` | `1ac411d127a35bb2d287f39e3a4084ed1407884e52377a5cd9bda1a3ea7ef47b` |
| `assets/agents/devin.png` | Devin | Cognition AI, Inc. | `light/devin-color.png` | `b9eef69b9ff7ba543a0e2ac13783aeee896b9b0fdc023372a68ae930a6a80061` |
| `assets/agents/gemini.png` | Gemini CLI | Google LLC | `light/gemini-color.png` | `10c628f55d22a9725b9f9fccce7cf062b9fb68da5f7736e87010e0594d7ba6db` |
| `assets/agents/grok.png` | Grok | xAI | xAI's logo pack, `Grok_Logomark_Dark.png` (see below) | `37ddbcb6e2a7f2e4b3be78a7d41296a3bc7edf6926362434efc00df5a56a3586` |
| `assets/agents/dark/grok.png` | Grok | xAI | xAI's logo pack, `Grok_Logomark_Light.png` (see below) | `359056ee8983cfa0ba7e72795078c7c0ddf6c5d7a1870401ab960ed4f9df9e53` |
| `assets/agents/hermes.png` | Hermes Agent | Nous Research, Inc. | `light/hermesagent.png` | `a4eacd7022af9551688a573cb2cccc5baa3ceb5fcb22728658ce444ef7449567` |
| `assets/agents/dark/hermes.png` | Hermes Agent | Nous Research, Inc. | `dark/hermesagent.png` | `c4e083a6ddda25f3ffbc74a4754cef020677f73ed324ec46a4c801e1f472767d` |
| `assets/agents/kilo.png` | Kilo | Kilo Code | Kilo's repository (see below) | `fa0f39f2409d31fd5e5b132fc9f533d57c879f6e22bdd0f21e250cd987b2ffeb` |
| `assets/agents/kimi.png` | Kimi Code | Moonshot AI | `light/kimi.png` | `ae68c5f479c6b92bc79f56172f2bb789c50e46c69def7a443209555086acddc3` |
| `assets/agents/dark/kimi.png` | Kimi Code | Moonshot AI | `dark/kimi.png` | `373db4703eb8b6e5a2e165c49708bb567ef29bb210f63ee8216f5c9ee1cb66ba` |
| `assets/agents/kiro.png` | Kiro | Amazon.com, Inc. or its affiliates | `light/kiro-color.png` | `98922033d598cf8eb280c0deb7088ecd04608517909d7e15e1f5c1ba5a22896b` |
| `assets/agents/mastracode.png` | Mastra Code | Kepler Software, Inc. (Mastra) | `light/mastra.png` | `0633fa9ee8f1735034f780916c11520e941b82d4fa205499b018354ecbd5a813` |
| `assets/agents/dark/mastracode.png` | Mastra Code | Kepler Software, Inc. (Mastra) | `dark/mastra.png` | `b9da24be9a982232596bc15b0cafcfc841134194eedafa5181e0a88d723fad1e` |
| `assets/agents/opencode.png` | opencode | Anomaly Innovations, Inc. | `light/opencode.png` | `8dd736cfa2628863a6e008a134d2094da69727d7359caca5bd05fe322ab13905` |
| `assets/agents/dark/opencode.png` | opencode | Anomaly Innovations, Inc. | `dark/opencode.png` | `017ff9bc277303d042baa12017be114f876b045260603528f5a7beedfdd6c714` |
| `assets/agents/pi.png` | pi | the pi project (pi.dev) | `light/pi.png` | `debf685b16716a4df4b212185c58416cf6a1eecfec7a8b69f4f9421e7e707b48` |
| `assets/agents/dark/pi.png` | pi | the pi project (pi.dev) | `dark/pi.png` | `0a64d7f14c865f8bac41f93952bccd23680bc20a77680a7c2729a81cd80526c4` |
| `assets/agents/qodercli.png` | Qoder CLI | Qoder (qoder.com) | `light/qoder-color.png` | `707778741a50bac7bf8d8153b466f04a23fd404b5d8d1e5cfdb01b2a76a66196` |
| `assets/agents/dark/qodercli.png` | Qoder CLI | Qoder (qoder.com) | `dark/qoder-color.png` | `8930cabad212df7f7b0ebeb68af14f8dbbca6213c80a86f457f40bf158fc513b` |
| `assets/agents/qwen.png` | Qwen Code | Alibaba Group | `light/qwen-color.png` | `de9bc7e285164e0d284a7b9555511c7f7767af7699d6272b3b67b376eeddbfb3` |

**Grok:** from xAI's logo pack,
<https://data.x.ai/logos/SpaceXAI_Grok_Assets.zip> (sha256
`db9129acd4efc4c2202d25afe31b70281a79f8507f75520ab5e6b3356895a7e9`), linked from
<https://x.ai/legal/brand-guidelines>, used as provided there.

**Kilo:** `packages/kilo-vscode/assets/icons/kilo-light.png` from
<https://github.com/Kilo-Org/kilocode> at commit
`7d977bce994af36f0edf752cb53e3aefc7aeb214`, one of the logos Kilo's
[ecosystem page](https://kilo.ai/docs/contributing/ecosystem) invites others
to use from its open-source repositories.

### LobeHub's licence

This covers LobeHub's drawings. It does not grant any right in the marks
themselves, which stay with their owners. The npm package ships without a
licence file; this text is from
<https://github.com/lobehub/lobe-icons/blob/v1.97.1/LICENSE>.

```
MIT License

Copyright (c) 2023 LobeHub

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## terminal-notifier

`vendor/HerdrNudge.app` is terminal-notifier 3.1.0 with our own bundle id,
name and icon. Its MIT licence is in `vendor/terminal-notifier-LICENSE.md`.
