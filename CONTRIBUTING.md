# Contributing Guide

Thanks for wanting to improve FXServer Installer. This project is still new and actively changing, so the most helpful contributions are focused, tested, and easy to review.

## Before You Start

Before opening an issue or pull request:

1. Search existing issues and pull requests.
2. Check whether the issue already exists under a different title.
3. Do not open duplicate issues for the same bug or request.
4. Do not open a pull request for a large feature without first checking whether it fits the project direction.
5. Keep one pull request focused on one feature, fix, or cleanup.

If you are unsure whether a change belongs in the project, open an issue first and describe what you want to build.

## Reporting Bugs

Good bug reports make the issue easier to reproduce and fix. Include:

- App version.
- Windows version.
- Whether you used the installer or a source build.
- What you expected to happen.
- What actually happened.
- Exact error messages.
- Steps to reproduce the issue.
- Relevant screenshots or short screen recordings for UI problems.
- Relevant Application Logs, Server Logs, MariaDB installer logs, or console output.

Do not include secrets. Redact values such as:

- `rcon_password`.
- Database passwords.
- CFX keys.
- Private IP addresses.
- Private server names if needed.
- Any tokens, API keys, or credentials.

## Requesting Features

For feature requests, explain:

- The workflow you are trying to improve.
- Who benefits from the change.
- What the current workaround is.
- Any examples, screenshots, or references that make the idea clearer.

Avoid opening several issues for tiny variations of the same idea. Group related suggestions when they belong to the same workflow.

## Local Setup

Install the project dependencies:

```bash
npm ci
```

Start the development app:

```bash
npm run tauri dev
```

Run frontend and TypeScript checks:

```bash
npm run check
```

Run Rust checks:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Build a release locally:

```bash
npm run tauri build
```

## Project Structure

Useful areas:

- `src/lib/features`: Svelte feature pages and UI-specific logic.
- `src/lib/components`: Shared UI and layout components.
- `src/lib/modules`: Frontend wrappers around Tauri commands.
- `src-tauri/src/commands`: Tauri command handlers exposed to the frontend.
- `src-tauri/src/services`: Backend service logic.
- `docs`: Longer documentation.
- `scripts`: Maintenance and build helper scripts.

Prefer existing patterns over introducing new architecture. Keep changes small unless the feature truly requires a broader change.

## UI Contributions

When changing UI:

- Prefer existing shadcn-svelte components where they fit.
- Match the current dark, compact desktop-app style.
- Keep controls ergonomic for repeated use.
- Avoid oversized marketing-style layouts inside tool pages.
- Make sure text fits at common desktop widths.
- Include screenshots or a short recording in the pull request.

For interactive or performance-sensitive pages, test with realistic data when possible.

## Backend Contributions

When changing Tauri/Rust code:

- Keep Windows behavior in mind.
- Avoid blocking the UI thread with long-running work.
- Log useful operation details to Application Logs when it helps users debug a failure.
- Be careful around installer, service-control, database, file-writing, and process-spawning code.
- Do not remove data unless the user explicitly asked for that behavior.

For MariaDB changes, always consider data preservation and backup warnings.

## Conventional Commits

Use conventional commits for every commit:

```text
<type>(optional-scope): <short description>
```

Examples:

```text
feat(resource-manager): add reinstall warning
fix(mariadb): preserve HeidiSQL during uninstall
docs: add source build guide
perf(console): batch terminal output rendering
chore(release): add Windows publish workflow
```

Common types:

- `feat`: A new feature.
- `fix`: A bug fix.
- `docs`: Documentation-only changes.
- `style`: Formatting-only changes with no behavior change.
- `refactor`: Code restructuring without behavior change.
- `perf`: Performance improvements.
- `test`: Tests or test utilities.
- `build`: Build system, dependencies, or packaging.
- `ci`: CI/workflow changes.
- `chore`: Maintenance work that does not fit the other types.

Guidelines:

- Use lowercase type names.
- Keep the first line short and specific.
- Use a scope when it makes the affected area obvious.
- Use the imperative mood when possible, for example `fix`, `add`, `update`, `remove`.
- Do not use vague messages like `updates`, `fix stuff`, or `changes`.

For breaking changes, include a footer:

```text
feat(config): change profile discovery format

BREAKING CHANGE: txData profile discovery now requires config.json to include server.dataPath.
```

## Pull Request Checklist

Before opening a pull request:

- Search for duplicate pull requests.
- Rebase or merge the latest `main`.
- Keep the pull request focused.
- Run `npm run check`.
- Run relevant Rust checks when backend code changed.
- Test the feature manually when it touches app behavior.
- Add or update docs when behavior changes.
- Include screenshots or recordings for UI changes.
- Explain what you changed and why.
- List the verification commands you ran.

Do not include unrelated refactors just because you noticed them while working on a feature.

## What Not To Commit

Do not commit:

- `node_modules`.
- `dist`.
- Tauri build output.
- Generated installers.
- Local logs.
- Database dumps.
- `.env` files.
- Secrets or tokens.
- Machine-specific paths unless they are clearly examples in documentation.

## Versioning And Releases

Maintainers should bump versions before publishing a release:

```bash
npm run version:bump -- patch
```

Use `patch`, `minor`, `major`, or an explicit version. The release workflow reads the Tauri version and creates the matching GitHub tag/release.

Most pull requests do not need to bump the version unless they are specifically preparing a release.

## Review Expectations

Reviews focus on correctness, safety, user experience, performance, and maintainability. A reviewer may ask for smaller changes, clearer naming, more testing, or documentation updates.

Please be patient during review. The goal is to keep the app useful without making it fragile.
