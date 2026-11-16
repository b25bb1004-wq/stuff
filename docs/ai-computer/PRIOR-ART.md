# Who has built computers and tools for AI, and what to take from them (scanned 2026-11-16)

Every row is from web search summaries only [search]; open the source before quoting a number.

| Work | What they did | What Monolith takes |
|---|---|---|
| SWE-agent, Yang et al., arXiv 2405.15793 (NeurIPS 2024) | Named the agent-computer interface. Few simple commands, short feedback, guardrails. The edit command that lints before saving mattered most. The interface alone gave a large gain over a raw shell | Design each tool's agent surface on purpose. Edits validate before they land. Short, exact feedback |
| Anthropic, "Writing effective tools for agents" (2025) | Namespaced tools, token-efficient results, errors that say how to fix the call, tools tuned with evals run by agents | Tool names like `chronos_status`, paging, teaching errors, and a suite of agent tasks as the gate |
| Playwright MCP (Microsoft) | Gives the model the page's accessibility tree as text with a short reference per element. No pixels, no vision model | Odyssey `snapshot`: role, name and ref per element. Screenshot only as a fallback |
| Web Bot Auth (IETF draft, Cloudflare and Google authors; Cloudflare "signed agents") | Agents sign each HTTP request (RFC 9421, Ed25519) and publish their key at a well-known URL, so sites can verify who is calling | The legitimate answer to bot walls. Sphinx already has Ed25519. Needs a domain for the key and registration by the operator |
| AIOS, Mei et al., arXiv 2403.16971 | An "agent OS" kernel: scheduler for agent requests, context, memory, storage, tool and access managers | The list of kernel services Titan needs for agents. Kairos, Midas, Hades and Iridium map onto it |
| agentOS (Rivet), aaos, Windows Agent Runtime (2026, as reported) | Deny-by-default capability grants per agent: files, network, processes | A capability manifest per agent session, enforced in the `monolith` door and in Pandora |
| Model Context Protocol | One JSON-RPC protocol for tools, resources and prompts | The door speaks MCP, so Claude Code and other agents connect without glue |

Not found in this scan: undo across files, database and shell in one step; tools that are agent-native from
the kernel up and written from scratch; a human view of live agent work with rewind.
"Not found" means six searches, not a review.

Sources:
- https://arxiv.org/abs/2405.15793
- https://arxiv.org/abs/2403.16971
- https://anthropic.com/engineering/writing-tools-for-agents
- https://playwright.dev/mcp/introduction
- https://datatracker.ietf.org/doc/html/draft-ietf-webbotauth-httpsig-protocol
- https://rivet.dev/changelog/2026-04-04-introducing-agentos/
