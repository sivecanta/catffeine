# Skill Mechanics (Antigravity)

The skill-specific branch of [`writing-for-agents`](./SKILL.md): what changes when the document is an Antigravity skill in `.agents/skills/<name>/SKILL.md` — frontmatter, progressive disclosure, and checklist formatting. Everything else about writing it is the universal reference in `SKILL.md`.

## Directory & Frontmatter

Every skill lives at `.agents/skills/<name>/SKILL.md` and begins with YAML frontmatter:

```yaml
---
name: my-skill-name
description: >-
  Third-person description of what this skill does and when the agent should
  activate it.
---
```

- `name` (required): lowercase, hyphenated identifier matching the folder name.
- `description` (required): the top-level **context pointer** injected into `<skills>` in every conversation turn. Because it is always loaded, apply the pointer-writing rules from `SKILL.md` strictly: front-load leading words, state one trigger per branch, and cut filler.

## Progressive Disclosure in Antigravity

Antigravity injects only `name` and `description` into the initial context window. When a skill is relevant, the agent reads `SKILL.md` on demand.

Use subdirectories (`references/`, `scripts/`, `examples/`, `resources/`) or sibling `.md` files for detailed reference material that only specific branches of the skill need, linking to them with relative links (e.g. `[tests.md](./tests.md)`).

## Copied Checklists

When a skill tells the agent to copy a checklist into its reply, write `- [ ] Plan`, not `- [ ] 1. Plan`. Markdown renderers treat `1.` inside a task-list item as a nested ordered list. Numbering belongs on the section headings (`## 1. Plan`).
