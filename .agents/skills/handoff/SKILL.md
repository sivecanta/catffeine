---
name: handoff
description: >-
  Compact the current conversation into a handoff document for another agent
  session to pick up. Use when the user asks for a handoff summary or wants to
  continue the current work in a fresh session.
---

# Handoff

Write a handoff document summarising the current conversation so a fresh agent session can continue the work seamlessly. Save it to the conversation's artifact/scratch directory or the OS temporary directory — not inside the project repository unless the user explicitly asks.

Include:
1. **Goal & Current Status**: What was requested, what is complete, and what remains open.
2. **Key Decisions & Context**: Architectural choices settled, tricky OS/Tauri edge cases discovered, and files touched.
3. **Suggested Skills**: Which workspace skills in `.agents/skills/` (`ponytail`, `tdd`, `diagnosing-bugs`, `grilling`, `planner`, `worker`, `finisher`) the next agent should activate first.
4. **Next Steps**: Concrete, ordered actions for the next session.

Rules:
- Do not duplicate content already captured in existing artifacts, plans (`docs/plans/`), specs (`README.md`, `PROMPT.md`), commits, or diffs. Reference them by file path or commit hash instead.
- Redact any sensitive information (tokens, credentials, or personal data).
- If the user specifies what the next session will focus on, tailor the handoff document around that focus.
