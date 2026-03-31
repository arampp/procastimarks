---
name: request-copilot-review
description: Request a GitHub Copilot code review on a pull request using the GitHub CLI
license: MIT
compatibility: opencode
---

## What I do

Add GitHub Copilot as a reviewer to an existing pull request using the correct
`gh` CLI command (requires gh v2.88.0 or later).

## When to use me

Use this when the user asks to add `@copilot` as a reviewer to a pull request,
or to "request a Copilot review".

## Steps

1. Confirm the PR number (use `gh pr view` to find it if not provided).
2. Check that the `gh` CLI is v2.88.0 or later:

   ```
   gh --version
   ```

3. Add Copilot as a reviewer:

   ```
   gh pr edit <PR_NUMBER> --add-reviewer @copilot
   ```

   If no PR number is supplied, `gh pr edit` without a number targets the PR
   for the current branch.

## Notes

- The `--reviewer copilot` flag (without `@`) and the GitHub REST/GraphQL
  reviewer API do **not** work for Copilot — they resolve a human login, not
  the Copilot bot. Always use `@copilot` with `gh pr edit`.
- A `@copilot review` comment on the PR also triggers a review, but
  `gh pr edit --add-reviewer @copilot` is the canonical CLI approach since
  gh v2.88.0.
- Reference: https://github.blog/changelog/2026-03-11-request-copilot-code-review-from-github-cli/
