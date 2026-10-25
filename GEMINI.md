# Catffeine — Project Guidelines

## Commit Conventions

All git commits **must** follow [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/):

```text
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

- **Format**: `<type>(<scope>): <description>` — lowercase, imperative mood, no trailing period.
- **Common types**: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, `chore`, `style`.
- **Breaking changes**: Append `!` after the type/scope (`feat(watcher)!: ...`) or include a `BREAKING CHANGE:` footer.

## Verification & Checks

Before committing code changes, ensure all checks are green:

- **Frontend**: `npm run build` (`tsc && vite build`)
- **Rust Backend**: `cargo test --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
- **Windows Release Binary (from WSL)**: `npm run build:win`
