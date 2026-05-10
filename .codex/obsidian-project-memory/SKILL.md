---
name: obsidian-project-memory
description: Use when working on KGet or other projects connected to the Segundo Cerebro Obsidian vault, especially before implementing features, refactors, architectural changes, roadmap work, or important fixes. Read the vault context first and update the relevant Obsidian notes after significant implementation discoveries, new features, architecture decisions, or changed project conventions.
---

# Obsidian Project Memory

Use the Obsidian vault as external project memory. Keep the agent grounded in the latest project context before coding, and keep the vault current after meaningful work.

## Vault

```text
/Volumes/Davi SSD 1/SegundoCerebroObsidian/Segundo Cerebro dev
```

Always start by reading:

```text
_context/codex-context.md
```

For KGet work, also read:

- `_context/Project Index - KGet.md`
- `02 - Arquitetura/Arquitetura - KGet.md`
- `03 - Dominios/Dominio - Download Manager.md` when domain concepts, jobs, queue, protocols, or events are involved.
- `05 - Decisoes/Roadmap - KGet.md` when prioritizing or planning future-facing work.

For OrcamentaAI work, follow the links in `_context/codex-context.md` and `_context/Project Index - OrcamentaAI.md`.

## Before Implementing

1. Read `_context/codex-context.md`.
2. Identify the active project from the current repository path or user request.
3. Read that project's index and the most relevant architecture/domain notes.
4. Let the vault guide naming, boundaries, verification commands, and architectural direction.
5. Prefer repository source files over vault notes when they disagree; then update the vault if the source reveals newer truth.

## During Implementation

- Treat vault notes as navigation and memory, not as a substitute for reading code.
- Keep implementation aligned with documented project direction.
- If the work reveals an architectural decision, changed convention, hidden dependency, risky edge case, or updated command, remember to update the vault before finishing.
- Avoid adding noisy progress logs to the vault. Capture durable context only.

## After Significant Work

Update the vault when any of these happen:

- A new feature changes user-facing behavior.
- A refactor changes module boundaries, contracts, or ownership.
- A new command, test strategy, build step, dependency, or release process appears.
- A bug fix reveals an invariant or edge case future agents should know.
- A roadmap item becomes done, obsolete, or needs reprioritization.
- The code contradicts existing vault notes.

For KGet, update one or more of:

- `_context/Project Index - KGet.md` for quick agent context, commands, entry points, or high-level status.
- `01 - Projetos/KGet/KGet.md` for project narrative and durable overview.
- `02 - Arquitetura/Arquitetura - KGet.md` for module boundaries, contracts, events, frontends, or refactors.
- `03 - Dominios/Dominio - Download Manager.md` for download jobs, states, protocols, integrity, queue, or invariants.
- `05 - Decisoes/Roadmap - KGet.md` for priorities and completed/planned work.
- `00 - Mapas/Mapa Mental - KGet.md` or `00 - Mapas/KGet.canvas` when navigation changes meaningfully.

## Update Style

- Append or revise concise, durable notes.
- Prefer links using Obsidian wikilinks like `[[Arquitetura - KGet]]`.
- Do not paste large code snippets unless the exact snippet is a stable contract.
- Date decisions only when chronology matters.
- Keep Portuguese as the default language for vault notes unless the target note is already English.
- If a note is stale, update it directly instead of adding a contradictory note elsewhere.

## Final Response

Mention vault updates briefly in the final answer, including the note names changed. If no vault update was needed after implementation, say that the work did not add durable architectural context.
